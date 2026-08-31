/**
 * Defines the Browse View React component and related UI helpers.
 */

import React, { useState, useEffect, useLayoutEffect, useCallback, useMemo, useRef } from 'react';
import { api } from '../api';
import type { Artist, Album, ClientEntityId, Track, Genre, Library, LastFmInfo, SimilarArtist, ArtistMergeInfo, UnmergeResult } from '../types';
import type { EntityId } from '../entityId';
import { KebabButton } from './ContextMenu';
import MetadataRefreshModal from './MetadataRefreshModal';
import MetadataEditModal from './MetadataEditModal';
import MergeArtistsModal from './MergeArtistsModal';
import UnmergeModal from './UnmergeModal';
import { useAdaptiveAccentEnabled } from '../hooks/useAdaptiveAccent';
import { useScanActivityRefresh } from '../hooks/useScanActivityRefresh';
import { groupArtistDiscographyByReleaseType } from '../releaseTypes';
import { findTopTrackMatch, matchesTrackArtist, resolveTopTrackFromLibrarySearch } from '../artistTrackMatching';
import ArtImage from './ArtImage';
import StarRating from './StarRating';
import { phase2 } from '../uiPhase2';
import { HYBRID_ARTWORK_HOVER, hybridBrowseStyles } from '../hybridPreview';

const ALPHA_RAIL_LETTERS = ['#', ...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('')];
const ROOT_SCROLL_BY_VIEW: Record<string, number> = {};
const ROOT_ANCHOR_BY_VIEW: Record<string, ClientEntityId> = {};

/** True for the compilation pseudo-artist — excluded from merge selection
 * eligibility (artist consolidation, plan §8 decision #4). */
export function isVariousArtistsName(name: string | null | undefined): boolean {
  return (name ?? '').trim().toLowerCase() === 'various artists';
}

export function toAlphaBucket(raw: string | null | undefined): string {
  const normalized = (raw ?? '').trim();
  if (!normalized) return '#';
  const first = normalized[0].toUpperCase();
  return first >= 'A' && first <= 'Z' ? first : '#';
}

export function buildLetterFirstIndexMap<T>(
  items: T[],
  getName: (item: T) => string | null | undefined,
): Record<string, number> {
  const map: Record<string, number> = {};
  items.forEach((item, idx) => {
    const letter = toAlphaBucket(getName(item));
    if (map[letter] === undefined) {
      map[letter] = idx;
    }
  });
  return map;
}

// ─── Row virtualization (root Browse windowing, plan Phase 2) ─────────────────
// True windowing for the root album grid/list: only rows near the viewport are
// mounted, so React tree size stays proportional to viewport size rather than
// collection size. See wip/large-dataset-album-artwork-performance-plan.md §7.
//
// If the scroll container's measured width is 0 (not yet laid out — e.g. jsdom
// tests without a ResizeObserver-driven layout, or a container hidden at mount),
// callers must render the full unwindowed list instead of guessing a window from
// unknown dimensions. This keeps existing non-visual tests unaffected and only
// activates windowing once a container has been really measured.

const GRID_OVERSCAN_ROWS = 3;
const LIST_OVERSCAN_ROWS = 6;
// Below this size, always use the plain unwindowed render — small libraries
// don't have a DOM-size problem, and it keeps the (still DOM-measurement-based)
// fallback path simple. At or above it, ALWAYS use the windowed algorithm, even
// before the container's real size has been measured (using degenerate
// defaults — 1 column, 0 viewport height — until useLayoutEffect corrects
// them). This is what actually matters: a purely props-driven `windowed` flag
// (not one that waits on state from an effect) guarantees a large collection
// is never rendered in full, not even for the transient first render pass
// before measurement lands — which a measurement-only gate cannot guarantee,
// since React's very first render always runs before any effect has fired.
const WINDOWING_SIZE_THRESHOLD = 200;

/** Number of grid columns for a given scroll-container width, replicating the
 * `repeat(auto-fill, minmax(minTile, 1fr))` CSS grid track algorithm so the
 * windowed layout matches what CSS auto-fill would have produced. */
export function computeGridColumns(containerWidth: number, minTile: number, gap: number, paddingX: number): number {
  const available = containerWidth - paddingX * 2;
  if (available <= 0) return 1;
  const cols = Math.floor((available + gap) / (minTile + gap));
  return Math.max(1, cols);
}

/** Pixel width of one column once `columns` is known, matching the `1fr` track
 * sizing CSS grid would compute for the same container. */
export function computeGridTileWidth(containerWidth: number, columns: number, gap: number, paddingX: number): number {
  const available = containerWidth - paddingX * 2;
  const width = (available - gap * (columns - 1)) / columns;
  return Math.max(0, width);
}

/** Inclusive [startRow, endRow] of rows to mount for a scrolled window, with overscan. */
export function computeVisibleRowRange(
  scrollTop: number,
  viewportHeight: number,
  rowHeight: number,
  totalRows: number,
  overscanRows: number,
): [number, number] {
  if (totalRows <= 0 || rowHeight <= 0) return [0, -1];
  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - overscanRows);
  const last = Math.min(totalRows - 1, Math.ceil((scrollTop + viewportHeight) / rowHeight) + overscanRows);
  return [first, last];
}

/** Measures a scroll container's `clientWidth`/`clientHeight` (padding-box,
 * matching the CSS `padding` on the same element), synchronously as soon as
 * the element is attached (before paint, so real layouts never flash a full
 * unwindowed render first) and on every resize thereafter. Returns `{0, 0}`
 * until a real measurement is available.
 *
 * Takes an existing stable ref object (kept for every other imperative use
 * already in this file — scroll manipulation, event listeners, etc.) and
 * returns an `attachRef` callback to place on the JSX element *instead of*
 * that ref directly. This is not cosmetic: `AlbumGrid`/`AlbumList` both have
 * an early return (`if (!albums.length) return <div>No albums found</div>`)
 * *after* their hooks — so on first mount, before the root Browse fetch has
 * resolved, `albums` is still `[]` and the real scroll container never
 * renders at all that pass. A plain `useLayoutEffect(..., [ref])` already
 * ran once against `ref.current === null` on that pass and, since a `useRef`
 * object's identity never changes, never runs again — so once the container
 * *does* mount (album data arrives, a later render takes the other branch),
 * nothing ever measures it and windowing is stuck computing from `{0, 0}`
 * forever. Routing attachment through `useState` instead makes "the element
 * showed up" a real dependency change the effect below reacts to. */
function useContainerSize(stableRef: React.RefObject<HTMLElement | null>): {
  size: { width: number; height: number };
  attachRef: (el: HTMLElement | null) => void;
} {
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [node, setNode] = useState<HTMLElement | null>(null);

  const attachRef = useCallback((el: HTMLElement | null) => {
    stableRef.current = el;
    setNode(el);
  }, [stableRef]);

  useLayoutEffect(() => {
    if (!node) return;
    const measure = () => {
      setSize((prev) => {
        const width = node.clientWidth;
        const height = node.clientHeight;
        return prev.width === width && prev.height === height ? prev : { width, height };
      });
    };
    measure();
    // Belt-and-suspenders: the synchronous measurement above can still
    // occasionally run before an ancestor's layout has fully settled. A
    // one-time re-check on the next animation frame self-heals that.
    const raf = requestAnimationFrame(measure);
    if (typeof ResizeObserver === 'undefined') {
      return () => cancelAnimationFrame(raf);
    }
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => {
      cancelAnimationFrame(raf);
      observer.disconnect();
    };
  }, [node]);

  return { size, attachRef };
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/** Sort Artists is part of this module's public API. */
export function sortArtists(
  artists: import('../types').Artist[],
  dir: 'asc' | 'desc',
): import('../types').Artist[] {
  const copy = [...artists];
  copy.sort((a, b) => {
    const cmp = (a.name || '').localeCompare(b.name || '', undefined, { sensitivity: 'base' });
    return dir === 'asc' ? cmp : -cmp;
  });
  return copy;
}

export function parseSortDir(value: string | null): 'asc' | 'desc' {
  return value === 'desc' ? 'desc' : 'asc';
}

function safeLocalStorageGet(key: string): string | null {
  try {
    if (typeof localStorage === 'undefined' || typeof localStorage.getItem !== 'function') return null;
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function safeLocalStorageSet(key: string, value: string): void {
  try {
    if (typeof localStorage === 'undefined' || typeof localStorage.setItem !== 'function') return;
    localStorage.setItem(key, value);
  } catch {}
}

/** Sort Albums is part of this module's public API. */
export function sortAlbums(
  albums: import('../types').Album[],
  field: 'title' | 'year' | 'rating',
  dir: 'asc' | 'desc',
): import('../types').Album[] {
  const copy = [...albums];
  copy.sort((a, b) => {
    if (field === 'title') {
      const cmp = (a.title || '').localeCompare(b.title || '', undefined, { sensitivity: 'base' });
      return dir === 'asc' ? cmp : -cmp;
    } else if (field === 'year') {
      // Undated albums always sink to the bottom regardless of direction, so
      // they are compared by title rather than against each other numerically.
      const ay = a.year ?? null;
      const by = b.year ?? null;
      if (ay == null && by == null) {
        return (a.title || '').localeCompare(b.title || '', undefined, { sensitivity: 'base' });
      }
      if (ay == null) return 1;
      if (by == null) return -1;
      const cmp = ay - by;
      if (cmp !== 0) return dir === 'asc' ? cmp : -cmp;
      return (a.title || '').localeCompare(b.title || '', undefined, { sensitivity: 'base' });
    }
    const ar = a.rating;
    const br = b.rating;
    if (ar == null && br == null) {
      return (a.title || '').localeCompare(b.title || '', undefined, { sensitivity: 'base' });
    }
    if (ar == null) return 1;
    if (br == null) return -1;
    const cmp = ar - br;
    if (cmp !== 0) return dir === 'asc' ? cmp : -cmp;
    return (a.title || '').localeCompare(b.title || '', undefined, { sensitivity: 'base' });
  });
  return copy;
}

type RatingFilter = 'all' | 'rated' | 'unrated' | 'gte4' | 'gte3';
type TrackSortMode = 'album' | 'rating';
const RATING_FILTER_OPTIONS: Array<[RatingFilter, string]> = [
  ['all', 'All'],
  ['rated', 'Rated'],
  ['unrated', 'Unrated'],
  ['gte4', '4+'],
  ['gte3', '3+'],
];

export function matchesRatingFilter(rating: number | null | undefined, filter: RatingFilter): boolean {
  if (filter === 'all') return true;
  if (filter === 'rated') return rating != null;
  if (filter === 'unrated') return rating == null;
  if (filter === 'gte4') return (rating ?? 0) >= 4;
  if (filter === 'gte3') return (rating ?? 0) >= 3;
  return true;
}

export function getRatingFilterLabel(filter: RatingFilter): string {
  return RATING_FILTER_OPTIONS.find(([value]) => value === filter)?.[1] ?? 'All';
}

export function getAlbumSortLabel(field: 'title' | 'year' | 'rating', dir: 'asc' | 'desc'): string {
  const label = field === 'title' ? 'Name' : field === 'year' ? 'Year' : 'Rating';
  return `${label} ${dir === 'asc' ? '↑' : '↓'}`;
}

export function filterAlbumsByRating(albums: Album[], filter: RatingFilter): Album[] {
  return albums.filter((album) => matchesRatingFilter(album.rating ?? null, filter));
}

export function filterArtistsByRating(artists: Artist[], filter: RatingFilter): Artist[] {
  return artists.filter((artist) => matchesRatingFilter(artist.rating ?? null, filter));
}

export function sortTracks(tracks: Track[], mode: TrackSortMode, dir: 'asc' | 'desc'): Track[] {
  const copy = [...tracks];
  if (mode === 'album') {
    copy.sort((a, b) => {
      const ad = a.disc_number ?? 1;
      const bd = b.disc_number ?? 1;
      if (ad !== bd) return ad - bd;
      const at = a.track_number ?? Number.MAX_SAFE_INTEGER;
      const bt = b.track_number ?? Number.MAX_SAFE_INTEGER;
      if (at !== bt) return at - bt;
      return (a.title || a.file_name || '').localeCompare(b.title || b.file_name || '', undefined, { sensitivity: 'base' });
    });
    return copy;
  }
  copy.sort((a, b) => {
    const ar = a.rating;
    const br = b.rating;
    if (ar == null && br == null) {
      const at = a.track_number ?? Number.MAX_SAFE_INTEGER;
      const bt = b.track_number ?? Number.MAX_SAFE_INTEGER;
      if (at !== bt) return at - bt;
      return (a.title || a.file_name || '').localeCompare(b.title || b.file_name || '', undefined, { sensitivity: 'base' });
    }
    if (ar == null) return 1;
    if (br == null) return -1;
    const cmp = ar - br;
    if (cmp !== 0) return dir === 'asc' ? cmp : -cmp;
    const at = a.track_number ?? Number.MAX_SAFE_INTEGER;
    const bt = b.track_number ?? Number.MAX_SAFE_INTEGER;
    return at - bt;
  });
  return copy;
}

export function filterTracksByRating(tracks: Track[], filter: RatingFilter): Track[] {
  return tracks.filter((track) => matchesRatingFilter(track.rating ?? null, filter));
}

export function getAlbumDisplayArtist(album: Album, groupBy: 'artist' | 'album_artist'): string | null {
  return groupBy === 'album_artist'
    ? (album.album_artist || album.artist || null)
    : (album.artist || null);
}

/** Toggle Genre Selection is part of this module's public API. */
export function toggleGenreSelection(selectedGenres: string[], genre: string): string[] {
  const trimmed = genre.trim();
  if (!trimmed) return selectedGenres;
  const normalized = trimmed.toLowerCase();
  const exists = selectedGenres.some((value) => value.trim().toLowerCase() === normalized);
  if (exists) {
    return selectedGenres.filter((value) => value.trim().toLowerCase() !== normalized);
  }
  return [...selectedGenres, trimmed];
}

/** To Single Genre Selection is part of this module's public API. */
export function toSingleGenreSelection(genre: string): string[] {
  const trimmed = genre.trim();
  return trimmed ? [trimmed] : [];
}

/** Toggle Library Selection is part of this module's public API. */
export function toggleLibrarySelection(selectedLibraryIds: ClientEntityId[], libraryId: ClientEntityId): ClientEntityId[] {
  const normalized = String(libraryId).trim();
  if (!normalized) return selectedLibraryIds;
  if (selectedLibraryIds.some((id) => String(id) === normalized)) {
    return selectedLibraryIds.filter((id) => String(id) !== normalized);
  }
  return [...selectedLibraryIds, libraryId];
}

/** Should Apply Browse Root Fetch Result is part of this module's public API. */
export function shouldApplyBrowseRootFetchResult(fetchToken: number, activeToken: number): boolean {
  return fetchToken === activeToken;
}

export function fmtDur(s: number | null | undefined): string {
  if (!s) return '';
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = Math.floor(s % 60);
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${sec}s`;
  return `${sec}s`;
}

export function fmtTrackDur(s: number | null): string {
  if (!s) return '–';
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${String(sec).padStart(2, '0')}`;
}

// ─── Icons ────────────────────────────────────────────────────────────────────

export function matchesAlbumRatingTarget(candidate: Album, target: Album): boolean {
  if (candidate.id === target.id) return true;
  return (candidate.title ?? '').trim().toLowerCase() === (target.title ?? '').trim().toLowerCase()
    && (candidate.album_artist ?? '').trim().toLowerCase() === (target.album_artist ?? '').trim().toLowerCase();
}

export function applyAlbumRating(list: Album[], target: Album, rating: number | null): Album[] {
  return list.map((album) => (
    matchesAlbumRatingTarget(album, target)
      ? { ...album, rating }
      : album
  ));
}

/** Merge scan-discovered albums without replacing unchanged collection objects. */
export function mergeAlbumChanges(
  current: Album[],
  changes: Album[],
  groupBy: 'artist' | 'album_artist',
): Album[] {
  if (!changes.length) return current;
  const keyFor = (album: Album) => groupBy === 'album_artist'
    ? `${album.title}\u0000${album.album_artist ?? ''}`
    : String(album.id);
  const next = [...current];
  const indexes = new Map(next.map((album, index) => [keyFor(album), index]));
  for (const album of changes) {
    const key = keyFor(album);
    const index = indexes.get(key);
    if (index == null) {
      indexes.set(key, next.length);
      next.push(album);
    } else {
      next[index] = album;
    }
  }
  return next;
}

export function applyTrackRating(list: Track[], trackId: ClientEntityId, rating: number | null): Track[] {
  return list.map((track) => (track.id === trackId ? { ...track, rating } : track));
}

export function applyArtistRating(list: Artist[], artistId: ClientEntityId, rating: number | null): Artist[] {
  return list.map((artist) => (artist.id === artistId ? { ...artist, rating } : artist));
}

const ChevronRight = () => (
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
    <polyline points="9 18 15 12 9 6" />
  </svg>
);
const ChevronDown = () => (
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
    <polyline points="6 9 12 15 18 9" />
  </svg>
);
const BackIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
    <polyline points="15 18 9 12 15 6" />
  </svg>
);
const PlayIcon = ({ size = 12 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor">
    <polygon points="5,3 19,12 5,21" />
  </svg>
);
const AlbumIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
    <circle cx="12" cy="12" r="10" /><circle cx="12" cy="12" r="3" />
  </svg>
);
const ArtistIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
    <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" /><circle cx="12" cy="7" r="4" />
  </svg>
);

// ─── Breadcrumb ───────────────────────────────────────────────────────────────

function Breadcrumb({ items }: { items: { label: string; onClick: () => void }[] }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 6, padding: '10px 16px',
      borderBottom: '1px solid var(--border)', flexShrink: 0, flexWrap: 'wrap',
    }}>
      {items.map((item, i) => (
        <React.Fragment key={i}>
          {i > 0 && <span style={{ color: 'var(--text-muted)', fontSize: 11 }}>/</span>}
          <button
            onClick={item.onClick}
            style={{
              background: 'none', border: 'none', padding: '2px 6px', borderRadius: 4,
              color: i === items.length - 1 ? 'var(--text)' : 'var(--accent)',
              cursor: i === items.length - 1 ? 'default' : 'pointer',
              fontSize: 12, fontFamily: 'inherit', fontWeight: i === items.length - 1 ? 600 : 400,
            }}
          >
            {item.label}
          </button>
        </React.Fragment>
      ))}
    </div>
  );
}

function QuickJumpRail({
  availableLetters,
  activeLetter,
  onJump,
}: {
  availableLetters: Set<string>;
  activeLetter: string;
  onJump: (letter: string) => void;
}) {
  return (
    <div style={L.alphaRail} aria-label="Alphabet quick jump">
      {ALPHA_RAIL_LETTERS.map((letter) => {
        const enabled = availableLetters.has(letter);
        const active = enabled && activeLetter === letter;
        return (
          <button
            key={letter}
            style={{
              ...L.alphaRailLetter,
              ...(enabled ? {} : L.alphaRailLetterDisabled),
              ...(active ? L.alphaRailLetterActive : {}),
            }}
            onClick={() => enabled && onJump(letter)}
            disabled={!enabled}
            title={enabled ? `Jump to ${letter}` : `No entries for ${letter}`}
            aria-label={enabled ? `Jump to ${letter}` : `${letter} unavailable`}
          >
            {letter}
          </button>
        );
      })}
    </div>
  );
}

// ─── Artist List ──────────────────────────────────────────────────────────────

function ArtistList({
  artists, loading, onSelect, onPlay, alphabeticalJump = false, sortDir = 'asc', initialScrollTop = 0, onScrollTopChange,
  selectMode = false, selectedIds, onToggleSelect,
}: {
  artists: Artist[]; loading: boolean; onSelect: (a: Artist) => void; onPlay: (a: Artist) => void;
  alphabeticalJump?: boolean; sortDir?: 'asc' | 'desc';
  initialScrollTop?: number;
  onScrollTopChange?: (top: number) => void;
  /** Artist consolidation multi-select (see wip/artist-consolidation-implementation-plan.md). */
  selectMode?: boolean;
  selectedIds?: Set<ClientEntityId>;
  onToggleSelect?: (a: Artist) => void;
}) {
  const listRef = useRef<HTMLDivElement | null>(null);
  const anchorRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const [activeLetter, setActiveLetter] = useState('#');
  const [hoveredArtistId, setHoveredArtistId] = useState<ClientEntityId | null>(null);
  const letterFirstIndexMap = useMemo(() => buildLetterFirstIndexMap(artists, (artist) => artist.name), [artists]);
  const availableLetters = useMemo(() => new Set(Object.keys(letterFirstIndexMap)), [letterFirstIndexMap]);

  useEffect(() => {
    const container = listRef.current;
    if (!container) return;
    if (initialScrollTop > 0) container.scrollTop = initialScrollTop;
  }, [initialScrollTop, artists.length]);

  useEffect(() => {
    const container = listRef.current;
    if (!container) return;
    const onScroll = () => onScrollTopChange?.(container.scrollTop);
    container.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      container.removeEventListener('scroll', onScroll);
    };
  }, [onScrollTopChange]);

  useEffect(() => {
    if (!alphabeticalJump) return;
    const container = listRef.current;
    if (!container) return;
    let rafId = 0;
    const updateActiveLetter = () => {
      const y = container.scrollTop + 2;
      let nextActive: string | null = null;
      const letters = sortDir === 'desc' ? [...ALPHA_RAIL_LETTERS].reverse() : ALPHA_RAIL_LETTERS;
      for (const letter of letters) {
        const anchor = anchorRefs.current[letter];
        if (!anchor) continue;
        if (anchor.offsetTop <= y) nextActive = letter;
        else break;
      }
      if (!nextActive) {
        nextActive = letters.find((letter) => !!anchorRefs.current[letter]) ?? '#';
      }
      setActiveLetter((prev) => (prev === nextActive ? prev : nextActive!));
    };
    const onScroll = () => {
      onScrollTopChange?.(container.scrollTop);
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        updateActiveLetter();
      });
    };
    updateActiveLetter();
    container.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      container.removeEventListener('scroll', onScroll);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [alphabeticalJump, sortDir, artists, onScrollTopChange]);

  const jumpToLetter = useCallback((letter: string) => {
    const container = listRef.current;
    const anchor = anchorRefs.current[letter];
    if (!container || !anchor) return;
    container.scrollTo({ top: anchor.offsetTop, behavior: 'smooth' });
  }, []);

  if (loading && !artists.length) return <div style={L.empty}>Loading...</div>;
  if (!artists.length) return <div style={L.empty}>No artists found.</div>;

  return (
    <div style={L.alphaShellFill}>
      <div ref={listRef} style={{ ...L.list, ...(alphabeticalJump ? L.alphaScrollable : {}) }}>
        {artists.map((artist, i) => {
          const selectable = selectMode && !isVariousArtistsName(artist.name);
          const selected = selectMode && !!selectedIds?.has(artist.id);
          const handleActivate = () => {
            if (selectMode) {
              if (selectable) onToggleSelect?.(artist);
              return;
            }
            const container = listRef.current;
            if (container) onScrollTopChange?.(container.scrollTop);
            onSelect(artist);
          };
          return (
          <div
            key={artist.id}
            ref={letterFirstIndexMap[toAlphaBucket(artist.name)] === i
              ? (el) => { anchorRefs.current[toAlphaBucket(artist.name)] = el; }
              : undefined}
            style={{
              ...L.row,
              backgroundColor: selected
                ? 'color-mix(in srgb, var(--accent) 14%, var(--surface))'
                : i % 2 === 0 ? 'transparent' : 'rgba(255,255,255,0.018)',
              ...(selectMode && !selectable ? { opacity: 0.5 } : {}),
            }}
            onClick={handleActivate}
            onMouseEnter={() => setHoveredArtistId(artist.id)}
            onMouseLeave={() => setHoveredArtistId((prev) => (prev === artist.id ? null : prev))}
            title={selectMode && !selectable ? 'Not eligible for merging' : undefined}
          >
            {selectMode && (
              <div
                aria-hidden="true"
                style={{
                  width: 17, height: 17, borderRadius: 4, flexShrink: 0,
                  background: selected ? 'var(--accent)' : 'transparent',
                  border: `1.5px solid ${selected ? 'var(--accent)' : 'var(--border)'}`,
                  display: 'flex', alignItems: 'center', justifyContent: 'center',
                  opacity: selectable ? 1 : 0.45,
                }}
              >
                {selected && (
                  <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                )}
              </div>
            )}
            <div style={L.rowIcon}><ArtistIcon /></div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={L.primaryText}>
                {artist.name}
                {!!artist.metadata_locked && <LockBadge />}
              </div>
            </div>
            <div style={L.meta}>{artist.album_count} {artist.album_count === 1 ? 'album' : 'albums'}</div>
            <div style={L.meta}>{artist.track_count} tracks</div>
            {!selectMode && (
              <KebabButton
                target={{ kind: 'artist', artistId: artist.id, name: artist.name }}
                callbacks={{ onPlay: () => onPlay(artist), onOpen: () => onSelect(artist) }}
                visible={hoveredArtistId === artist.id}
              />
            )}
            <div style={L.chevron}><ChevronRight /></div>
          </div>
          );
        })}
      </div>
      {alphabeticalJump && (
        <QuickJumpRail availableLetters={availableLetters} activeLetter={activeLetter} onJump={jumpToLetter} />
      )}
    </div>
  );
}
// Root album list row height (must track `L.row`'s padding + content, see the
// `containIntrinsicSize: 'auto 46px'` size hint on that style — the row is a
// single fixed-height flex row, so this is exact, not an estimate).
const LIST_ROW_HEIGHT = 46;

/** Exported for direct windowing tests only (root Browse renders it internally). */
export function AlbumList({
  albums, loading, onSelect, onPlay, onQueue, showArtist = false, showThumbnail = false, groupBy = 'artist', fill = true, alphabeticalJump = false, initialScrollTop = 0, initialAnchorId, onScrollTopChange, onAnchorChange,
}: {
  albums: Album[]; loading: boolean;
  onSelect: (a: Album) => void;
  onPlay: (a: Album) => void;
  onQueue: (a: Album) => void;
  showArtist?: boolean;
  showThumbnail?: boolean;
  groupBy?: 'artist' | 'album_artist';
  fill?: boolean;
  alphabeticalJump?: boolean;
  initialScrollTop?: number;
  initialAnchorId?: ClientEntityId;
  onScrollTopChange?: (top: number) => void;
  onAnchorChange?: (id: ClientEntityId) => void;
}) {
  const listRef = useRef<HTMLDivElement | null>(null);
  const anchorRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const [activeLetter, setActiveLetter] = useState('#');
  const [hoveredAlbumId, setHoveredAlbumId] = useState<ClientEntityId | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [focusedIndex, setFocusedIndex] = useState<number | null>(null);
  const letterFirstIndexMap = useMemo(() => buildLetterFirstIndexMap(albums, (album) => album.title), [albums]);
  const availableLetters = useMemo(() => new Set(Object.keys(letterFirstIndexMap)), [letterFirstIndexMap]);

  // Only the root ("fill") usage is windowed — the embedded artist-detail
  // sections render a handful of albums in normal document flow (`fill=false`)
  // and are left exactly as before. Like AlbumGrid, windowing further requires
  // a real measured height (see useContainerSize); unmeasured environments
  // (tests, a hidden container at mount) fall back to the original unwindowed
  // render below.
  const { size: containerSize, attachRef: attachListRef } = useContainerSize(listRef);
  const windowed = fill && (containerSize.height > 0 || albums.length >= WINDOWING_SIZE_THRESHOLD);
  const totalRows = windowed ? albums.length : 0;
  const canvasHeight = windowed ? totalRows * LIST_ROW_HEIGHT : 0;

  useEffect(() => {
    const container = listRef.current;
    if (!container) return;
    if (initialScrollTop > 0) {
      container.scrollTop = initialScrollTop;
      setScrollTop(initialScrollTop);
    }
  }, [initialScrollTop, albums.length]);

  useEffect(() => {
    if (!initialAnchorId || windowed) return;
    const container = listRef.current;
    if (!container) return;
    const node = container.querySelector<HTMLElement>(`[data-root-anchor-id="${initialAnchorId}"]`);
    if (!node) return;
    if (typeof node.scrollIntoView === 'function') {
      node.scrollIntoView({ block: 'nearest' });
    }
  }, [initialAnchorId, albums.length, windowed]);

  useEffect(() => {
    if (!initialAnchorId || !windowed) return;
    const container = listRef.current;
    if (!container) return;
    const idx = albums.findIndex((a) => a.id === initialAnchorId);
    if (idx < 0) return;
    const top = idx * LIST_ROW_HEIGHT;
    container.scrollTop = top;
    setScrollTop(top);
  }, [initialAnchorId, albums, windowed]);

  useEffect(() => {
    const container = listRef.current;
    if (!container) return;
    const onScroll = () => onScrollTopChange?.(container.scrollTop);
    container.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      onScrollTopChange?.(container.scrollTop);
      container.removeEventListener('scroll', onScroll);
    };
  }, [onScrollTopChange]);

  useEffect(() => {
    if (!windowed) return;
    const container = listRef.current;
    if (!container) return;
    let rafId = 0;
    const onScroll = () => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        setScrollTop(container.scrollTop);
      });
    };
    container.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      container.removeEventListener('scroll', onScroll);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [windowed]);

  useEffect(() => {
    if (!alphabeticalJump || !fill || windowed) return;
    const container = listRef.current;
    if (!container) return;
    let rafId = 0;
    const updateActiveLetter = () => {
      const y = container.scrollTop + 2;
      let nextActive: string | null = null;
      for (const letter of ALPHA_RAIL_LETTERS) {
        const anchor = anchorRefs.current[letter];
        if (!anchor) continue;
        if (anchor.offsetTop <= y) nextActive = letter;
        else break;
      }
      if (!nextActive) {
        nextActive = ALPHA_RAIL_LETTERS.find((letter) => !!anchorRefs.current[letter]) ?? '#';
      }
      setActiveLetter((prev) => (prev === nextActive ? prev : nextActive!));
    };
    const onScroll = () => {
      onScrollTopChange?.(container.scrollTop);
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        updateActiveLetter();
      });
    };
    updateActiveLetter();
    container.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      container.removeEventListener('scroll', onScroll);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [alphabeticalJump, fill, albums, onScrollTopChange, windowed]);

  useEffect(() => {
    if (!alphabeticalJump || !windowed) return;
    const currentRow = Math.floor(scrollTop / LIST_ROW_HEIGHT);
    let nextActive: string | null = null;
    for (const letter of ALPHA_RAIL_LETTERS) {
      const idx = letterFirstIndexMap[letter];
      if (idx === undefined) continue;
      if (idx <= currentRow) nextActive = letter;
      else break;
    }
    if (!nextActive) {
      nextActive = ALPHA_RAIL_LETTERS.find((letter) => letterFirstIndexMap[letter] !== undefined) ?? '#';
    }
    setActiveLetter((prev) => (prev === nextActive ? prev : nextActive!));
  }, [alphabeticalJump, windowed, scrollTop, letterFirstIndexMap]);

  const jumpToLetter = useCallback((letter: string) => {
    const container = listRef.current;
    if (!container) return;
    if (windowed) {
      const idx = letterFirstIndexMap[letter];
      if (idx === undefined) return;
      container.scrollTo({ top: idx * LIST_ROW_HEIGHT, behavior: 'smooth' });
      return;
    }
    const anchor = anchorRefs.current[letter];
    if (!anchor) return;
    container.scrollTo({ top: anchor.offsetTop, behavior: 'smooth' });
  }, [windowed, letterFirstIndexMap]);

  if (loading && !albums.length) return <div style={L.empty}>Loading...</div>;
  if (!albums.length) return <div style={L.empty}>No albums found.</div>;

  const renderRow = (album: Album, i: number, extraStyle: React.CSSProperties | undefined, attachAnchorRef: boolean) => {
    const displayArtist = groupBy === 'album_artist'
      ? (album.album_artist || album.artist)
      : album.artist;

    return (
      <div
        key={album.id}
        ref={attachAnchorRef && fill && letterFirstIndexMap[toAlphaBucket(album.title)] === i
          ? (el: HTMLDivElement | null) => { anchorRefs.current[toAlphaBucket(album.title)] = el; }
          : undefined}
        data-root-anchor-id={attachAnchorRef ? album.id : undefined}
        style={{
          ...L.row,
          ...extraStyle,
          backgroundColor: hoveredAlbumId === album.id
            ? 'color-mix(in srgb, var(--accent) 14%, transparent)'
            : i % 2 === 0 ? 'transparent' : 'rgba(255,255,255,0.018)',
        }}
        onClick={() => {
          const container = listRef.current;
          if (container) onScrollTopChange?.(container.scrollTop);
          onAnchorChange?.(album.id);
          onSelect(album);
        }}
        onMouseEnter={() => setHoveredAlbumId(album.id)}
        onMouseLeave={() => setHoveredAlbumId((prev) => (prev === album.id ? null : prev))}
        onFocus={() => setFocusedIndex(i)}
        onBlur={() => setFocusedIndex((prev) => (prev === i ? null : prev))}
      >
        <div style={L.rowIcon}>
          {showThumbnail
            ? (
              <div style={L.rowThumb}>
                <AlbumTileImage albumId={album.id} title={album.title} />
              </div>
            )
            : <AlbumIcon />}
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={L.primaryText}>{album.title}</div>
          {showArtist && displayArtist && (
            <div style={L.secondaryText}>{displayArtist}</div>
          )}
        </div>
        {album.year && <div style={L.meta}>{album.year}</div>}
        <div style={L.meta}>{album.track_count} tracks{album.total_duration ? ` · ${fmtDur(album.total_duration)}` : ''}</div>
        <button
          style={L.playBtn}
          title="Play album"
          onClick={e => { e.stopPropagation(); onPlay(album); }}
        >
          <PlayIcon /> Play
        </button>
        <KebabButton
          target={{ kind: 'album', albumId: album.id, title: album.title }}
          callbacks={{ onPlay: () => onPlay(album), onQueue: () => onQueue(album) }}
          visible={hoveredAlbumId === album.id}
        />
        <div style={L.chevron}><ChevronRight /></div>
      </div>
    );
  };

  let listBody: React.ReactNode;
  if (windowed) {
    let [startRow, endRow] = computeVisibleRowRange(scrollTop, containerSize.height, LIST_ROW_HEIGHT, totalRows, LIST_OVERSCAN_ROWS);
    if (focusedIndex != null) {
      if (focusedIndex < startRow) startRow = focusedIndex;
      if (focusedIndex > endRow) endRow = focusedIndex;
    }
    const items: React.ReactNode[] = [];
    for (let i = startRow; i <= endRow; i++) {
      items.push(renderRow(albums[i], i, { position: 'absolute', top: i * LIST_ROW_HEIGHT, left: 0, right: 0 }, false));
    }
    listBody = <div style={{ position: 'relative', height: canvasHeight }}>{items}</div>;
  } else {
    listBody = albums.map((album, i) => renderRow(album, i, undefined, true));
  }

  return (
    <div style={fill ? L.alphaShellFill : L.alphaShellStatic}>
      <div
        ref={attachListRef}
        style={fill
          ? { ...L.list, ...(alphabeticalJump ? L.alphaScrollable : {}), ...(windowed ? { position: 'relative' } : {}) }
          : L.listStack}
      >
        {listBody}
      </div>
      {alphabeticalJump && fill && (
        <QuickJumpRail availableLetters={availableLetters} activeLetter={activeLetter} onJump={jumpToLetter} />
      )}
    </div>
  );
}
function AlbumCover({ albumId, title, refreshToken = 0, adaptiveAccentEnabled = false }: {
  albumId: ClientEntityId; title: string; refreshToken?: number;
  adaptiveAccentEnabled?: boolean;
}) {
  const [phase, setPhase] = useState<'loading' | 'image' | 'none'>('loading');
  const [imgEl, setImgEl] = useState<HTMLImageElement | null>(null);
  const artUrl = api.albumArtUrl(albumId, 800, refreshToken || undefined);
  useAdaptiveAccentEnabled(phase === 'image' ? artUrl : null, adaptiveAccentEnabled, imgEl);

  const SIZE = 160;

  return (
    <div style={{ ...L.coverBox, width: SIZE, height: SIZE, position: 'relative' }}>
      <ArtImage
        src={artUrl}
        alt={title}
        eager={true}
        fetchPriority="high"
        wrapperStyle={{ width: SIZE, height: SIZE }}
        fallback={(
          <div style={L.coverPlaceholder}>
            <AlbumIcon />
            {phase === 'none' ? (
              <div style={{ fontSize: 9, marginTop: 6, color: 'var(--text-muted)', textAlign: 'center', lineHeight: 1.4 }}>
                No cover found
              </div>
            ) : null}
          </div>
        )}
        imgStyle={{ width: SIZE, height: SIZE, objectFit: 'cover', borderRadius: 6, display: 'block' }}
        onImageReady={setImgEl}
        onLoadStateChange={(state) => setPhase(state === 'loaded' ? 'image' : state === 'error' ? 'none' : 'loading')}
      />
    </div>
  );
}

// ─── Last.fm Bio ──────────────────────────────────────────────────────────────

type LastFmState = 'idle' | 'loading' | 'ok' | 'error' | 'no-key';


interface LastFmTopTrack {
  name: string;
  playcount: number;
  listeners?: number;
  url?: string;
}

type ArtistPhotoPhase = 'loading' | 'local' | 'deezer' | 'spotify' | 'none';

function normalizeTrackText(value: string | null | undefined): string {
  return (value ?? '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, ' ')
    .trim();
}

/** Parse Last Fm Top Tracks is part of this module's public API. */
export function parseLastFmTopTracks(data: any, limit = 5): LastFmTopTrack[] {
  const rawTracks = data?.toptracks?.track;
  if (!Array.isArray(rawTracks)) return [];

  return rawTracks
    .map((track: any) => ({
      name: typeof track?.name === 'string' ? track.name.trim() : '',
      playcount: Number(track?.playcount),
      listeners: Number.isFinite(Number(track?.listeners)) ? Number(track.listeners) : undefined,
      url: typeof track?.url === 'string' ? track.url : undefined,
    }))
    .filter((track: LastFmTopTrack) => track.name && Number.isFinite(track.playcount) && track.playcount > 0)
    .sort((a: LastFmTopTrack, b: LastFmTopTrack) => b.playcount - a.playcount)
    .slice(0, limit);
}


/** Parse Artist Photo Payload is part of this module's public API. */
export function parseArtistPhotoPayload(data: any): { url: string; source: 'deezer' | 'spotify' } | null {
  if (typeof data?.url !== 'string' || !data.url) return null;
  const source = data?.source === 'spotify' ? 'spotify' : data?.source === 'deezer' ? 'deezer' : null;
  if (!source) return null;
  return { url: data.url, source };
}

function LastFmBio({ artist, album, description }: {
  artist: string;
  album?: string;
  description?: string | null;
}) {
  const [state, setState] = useState<LastFmState>('idle');
  const [info, setInfo]   = useState<LastFmInfo | null>(null);
  const [expanded, setExpanded] = useState(false);
  const hasCustomDesc = !!description?.trim();

  useEffect(() => {
    if (hasCustomDesc) return;
    if (!artist) { setState('idle'); return; }

    setState('loading');
    setInfo(null);
    setExpanded(false);

    api.lastfm.info(artist, album)
      .then(data => {
        setInfo(data);
        setState(data.summary || data.full ? 'ok' : 'error');
      })
      .catch((e: any) => {
        setState(e?.message === 'no-key' ? 'no-key' : 'error');
      });
  }, [artist, album, hasCustomDesc]);

  if (hasCustomDesc) return (
    <div style={L.bioWrap}>
      <div style={L.bioText}>{description!.trim()}</div>
    </div>
  );

  if (state === 'no-key') return (
    <div style={L.bioWrap}>
      <div style={L.bioNoKey}>
        Add a Last.fm API key in <strong>Settings</strong> to see artist & album info.
      </div>
    </div>
  );

  if (state === 'loading') return (
    <div style={L.bioWrap}>
      <div style={L.bioLoading}>
        <span style={L.bioLoadingDot} />
        Fetching from Last.fm…
      </div>
    </div>
  );

  if (state === 'error' || !info) return (
    <div style={L.bioWrap}>
      <div style={L.bioEmpty}>No info found on Last.fm</div>
    </div>
  );

  const text = expanded ? info.full : info.summary;

  return (
    <div style={L.bioWrap}>
      {/* Stats row */}
      {(info.listeners || info.playcount) && (
        <div style={L.bioStats}>
          {info.listeners && (
            <div style={L.bioStat}>
              <span style={L.bioStatVal}>{info.listeners}</span>
              <span style={L.bioStatLbl}>listeners</span>
            </div>
          )}
          {info.playcount && (
            <div style={L.bioStat}>
              <span style={L.bioStatVal}>{info.playcount}</span>
              <span style={L.bioStatLbl}>scrobbles</span>
            </div>
          )}
        </div>
      )}

      {/* Tags */}
      {info.tags && info.tags.length > 0 && (
        <div style={L.bioTags}>
          {info.tags.map(tag => (
            <span key={tag} style={L.bioTag}>{tag}</span>
          ))}
        </div>
      )}

      {/* Bio text */}
      {text && (
        <div style={L.bioText}>{text}</div>
      )}

      {/* Expand / Last.fm link */}
      <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginTop: 8, flexWrap: 'wrap' }}>
        {info.full && info.full !== info.summary && (
          <button
            style={L.bioExpandBtn}
            onClick={() => setExpanded(e => !e)}
          >
            {expanded ? 'Show less' : 'Read more'}
          </button>
        )}
        {info.url && (
          <a
            href={info.url}
            target="_blank"
            rel="noreferrer"
            style={L.bioLink}
          >
            Last.fm ↗
          </a>
        )}
      </div>
    </div>
  );
}

function LastFmTopTracks({
  artist, onPlayTopTracks, playLoading,
}: {
  artist: string;
  onPlayTopTracks: (tracks: LastFmTopTrack[]) => void;
  playLoading: boolean;
}) {
  const [state, setState] = useState<LastFmState>('idle');
  const [tracks, setTracks] = useState<LastFmTopTrack[]>([]);

  useEffect(() => {
    if (!artist) { setState('idle'); return; }

    setState('loading');
    setTracks([]);

    api.lastfm.topTracks(artist)
      .then(data => {
        const list = Array.isArray(data) ? data : [];
        setTracks(list);
        setState(list.length ? 'ok' : 'error');
      })
      .catch((e: any) => {
        setState(e?.message === 'no-key' ? 'no-key' : 'error');
      });
  }, [artist]);

  return (
    <div style={L.topTracksWrap}>
      <div style={L.topTracksHead}>
        <div style={L.topTracksTitle}>Top 5 Songs</div>
        <button
          style={{ ...L.btnSecondary, padding: '5px 10px', fontSize: 12, opacity: state === 'ok' && !playLoading ? 1 : 0.6 }}
          onClick={() => onPlayTopTracks(tracks)}
          disabled={state !== 'ok' || playLoading}
          title={state === 'ok' ? 'Play matched top tracks from your library' : 'No top tracks available'}
        >
          <PlayIcon /> {playLoading ? 'Building...' : 'Play Top 5'}
        </button>
      </div>
      {state === 'no-key' && <div style={L.topTracksHint}>Add a Last.fm API key in Settings to load top songs.</div>}
      {state === 'loading' && <div style={L.topTracksHint}>Loading Last.fm track data...</div>}
      {(state === 'error' || state === 'idle') && <div style={L.topTracksHint}>No Last.fm track stats found.</div>}
      {state === 'ok' && (
        <div style={L.topTracksList}>
          {tracks.map((track, idx) => (
            <div key={`${track.name}-${idx}`} style={L.topTrackRow}>
              <div style={L.topTrackRank}>{idx + 1}</div>
              <div style={{ minWidth: 0, flex: 1 }}>
                {track.url ? (
                  <a href={track.url} target="_blank" rel="noreferrer" style={L.topTrackLink}>{track.name}</a>
                ) : (
                  <div style={L.topTrackName}>{track.name}</div>
                )}
              </div>
              <div style={L.topTrackScrobbles}>{track.playcount.toLocaleString()} scrobbles</div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function ArtistPhoto({ artistId, artist, refreshToken = 0, adaptiveAccentEnabled = false }: {
  artistId: ClientEntityId;
  artist: string;
  refreshToken?: number;
  adaptiveAccentEnabled?: boolean;
}) {
  const [phase, setPhase] = useState<ArtistPhotoPhase>('loading');
  const [imgEl, setImgEl] = useState<HTMLImageElement | null>(null);
  const imgSrc = api.artistPhotoUrl(artistId, 800, refreshToken || undefined);
  useAdaptiveAccentEnabled(phase !== 'none' && phase !== 'loading' ? imgSrc : null, adaptiveAccentEnabled, imgEl);

  return (
    <div style={{ ...L.artistHeaderIcon, overflow: 'hidden', position: 'relative', padding: 0 }}>
      <ArtImage
        src={imgSrc}
        alt={artist}
        eager={true}
        fetchPriority="high"
        imgStyle={{ width: '100%', height: '100%', objectFit: 'cover' }}
        fallback={<div style={L.artistHeaderPlaceholder}><ArtistIcon /></div>}
        onImageReady={setImgEl}
        onLoadStateChange={(state) => {
          if (state === 'loaded') setPhase('local');
          else if (state === 'error') setPhase('none');
          else setPhase('loading');
        }}
      />
      {phase !== 'none' && phase !== 'loading' && (
        <div style={L.artistPhotoBadge}>
          {phase === 'local' ? 'Local' : phase === 'deezer' ? 'Deezer' : 'Spotify'}
        </div>
      )}
    </div>
  );
}

function ArtistTileImage({ artistId, artist }: { artistId: ClientEntityId; artist: string }) {
  return (
    <ArtImage
      src={api.artistPhotoUrl(artistId, 300)}
      alt={artist}
      imgStyle={L.gridArtImg}
      fallback={<div style={L.gridArtPlaceholder}><ArtistIcon /></div>}
    />
  );
}

function SimilarArtistTile({ artist, onSelect }: { artist: SimilarArtist; onSelect: (artist: Artist) => void }) {
  const [active, setActive] = useState(false);
  return (
    <button
      type="button"
      style={{ ...L.similarArtistCard, ...(active ? L.similarArtistCardActive : {}) }}
      onClick={() => onSelect(artist)}
      onMouseEnter={() => setActive(true)}
      onMouseLeave={() => setActive(false)}
      onFocus={() => setActive(true)}
      onBlur={() => setActive(false)}
    >
      <span style={L.similarArtistArt}>
        <ArtistTileImage artistId={artist.id} artist={artist.name} />
      </span>
      <span style={L.similarArtistName}>{artist.name}</span>
    </button>
  );
}

function SimilarArtistsSection({ artistId, onSelect }: {
  artistId: ClientEntityId;
  onSelect: (artist: Artist) => void;
}) {
  const [artists, setArtists] = useState<SimilarArtist[]>([]);

  useEffect(() => {
    let cancelled = false;
    setArtists([]);
    api.artistSimilar(artistId, 12)
      .then(response => {
        if (!cancelled) setArtists(response.artists);
      })
      .catch(() => {
        if (!cancelled) setArtists([]);
      });
    return () => {
      cancelled = true;
    };
  }, [artistId]);

  if (artists.length === 0) return null;

  return (
    <section style={L.similarArtistsSection}>
      <div style={L.sectionHeading}>similar artists</div>
      <div style={L.similarArtistsGrid}>
        {artists.map(artist => (
          <SimilarArtistTile key={artist.id} artist={artist} onSelect={onSelect} />
        ))}
      </div>
    </section>
  );
}

function AlbumTileImage({ albumId, title }: { albumId: ClientEntityId; title: string }) {
  return (
    <ArtImage
      src={api.albumArtUrl(albumId, 300)}
      alt={title}
      imgStyle={L.gridArtImg}
      fallback={<div style={L.gridArtPlaceholder}><AlbumIcon /></div>}
    />
  );
}

function ArtistGrid({
  artists, loading, onSelect, onPlay, alphabeticalJump = false, sortDir = 'asc', initialScrollTop = 0, initialAnchorId, onScrollTopChange, onAnchorChange, hybridPreview = false,
  selectMode = false, selectedIds, onToggleSelect,
}: {
  artists: Artist[]; loading: boolean; onSelect: (a: Artist) => void; onPlay: (a: Artist) => void;
  alphabeticalJump?: boolean; sortDir?: 'asc' | 'desc';
  initialScrollTop?: number;
  initialAnchorId?: ClientEntityId;
  onScrollTopChange?: (top: number) => void;
  onAnchorChange?: (id: ClientEntityId) => void;
  hybridPreview?: boolean;
  /** Artist consolidation multi-select (see wip/artist-consolidation-implementation-plan.md). */
  selectMode?: boolean;
  selectedIds?: Set<ClientEntityId>;
  onToggleSelect?: (a: Artist) => void;
}) {
  const gridRef = useRef<HTMLDivElement | null>(null);
  const anchorRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const [activeLetter, setActiveLetter] = useState('#');
  const [hoveredArtistId, setHoveredArtistId] = useState<ClientEntityId | null>(null);
  const letterFirstIndexMap = useMemo(() => buildLetterFirstIndexMap(artists, (artist) => artist.name), [artists]);
  const availableLetters = useMemo(() => new Set(Object.keys(letterFirstIndexMap)), [letterFirstIndexMap]);

  useEffect(() => {
    const container = gridRef.current;
    if (!container) return;
    if (initialScrollTop > 0) container.scrollTop = initialScrollTop;
  }, [initialScrollTop, artists.length]);
  useEffect(() => {
    if (!initialAnchorId) return;
    const container = gridRef.current;
    if (!container) return;
    const node = container.querySelector<HTMLElement>(`[data-root-anchor-id="${initialAnchorId}"]`);
    if (!node) return;
    if (typeof node.scrollIntoView === 'function') {
      node.scrollIntoView({ block: 'nearest' });
    }
  }, [initialAnchorId, artists.length]);

  useEffect(() => {
    const container = gridRef.current;
    if (!container) return;
    const onScroll = () => onScrollTopChange?.(container.scrollTop);
    container.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      container.removeEventListener('scroll', onScroll);
    };
  }, [onScrollTopChange]);

  useEffect(() => {
    if (!alphabeticalJump) return;
    const container = gridRef.current;
    if (!container) return;
    let rafId = 0;
    const updateActiveLetter = () => {
      const y = container.scrollTop + 2;
      let nextActive: string | null = null;
      const letters = sortDir === 'desc' ? [...ALPHA_RAIL_LETTERS].reverse() : ALPHA_RAIL_LETTERS;
      for (const letter of letters) {
        const anchor = anchorRefs.current[letter];
        if (!anchor) continue;
        if (anchor.offsetTop <= y) nextActive = letter;
        else break;
      }
      if (!nextActive) {
        nextActive = letters.find((letter) => !!anchorRefs.current[letter]) ?? '#';
      }
      setActiveLetter((prev) => (prev === nextActive ? prev : nextActive!));
    };
    const onScroll = () => {
      onScrollTopChange?.(container.scrollTop);
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        updateActiveLetter();
      });
    };
    updateActiveLetter();
    container.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      container.removeEventListener('scroll', onScroll);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [alphabeticalJump, sortDir, artists, onScrollTopChange]);

  const jumpToLetter = useCallback((letter: string) => {
    const container = gridRef.current;
    const anchor = anchorRefs.current[letter];
    if (!container || !anchor) return;
    container.scrollTo({ top: anchor.offsetTop, behavior: 'smooth' });
  }, []);

  if (loading && !artists.length) return <div style={L.empty}>Loading...</div>;
  if (!artists.length) return <div style={L.empty}>No artists found.</div>;

  return (
    <div style={L.alphaShellFill}>
      <div ref={gridRef} style={{ ...L.gridWrap, ...(alphabeticalJump ? L.alphaScrollable : {}) }}>
        {artists.map((artist, i) => {
          const selectable = selectMode && !isVariousArtistsName(artist.name);
          const selected = selectMode && !!selectedIds?.has(artist.id);
          const handleActivate = () => {
            if (selectMode) {
              if (selectable) onToggleSelect?.(artist);
              return;
            }
            const container = gridRef.current;
            if (container) onScrollTopChange?.(container.scrollTop);
            onAnchorChange?.(artist.id);
            onSelect(artist);
          };
          return (
          <div
            role="button"
            tabIndex={0}
            key={artist.id}
            ref={letterFirstIndexMap[toAlphaBucket(artist.name)] === i
              ? (el) => { anchorRefs.current[toAlphaBucket(artist.name)] = el; }
              : undefined}
            data-root-anchor-id={artist.id}
            style={{
              ...L.gridTileBtn,
              ...(hybridPreview
                ? {
                    backgroundColor: 'transparent',
                    borderColor: 'transparent',
                    boxShadow: 'none',
                  }
                : {
                    backgroundColor: selected
                      ? 'color-mix(in srgb, var(--accent) 14%, var(--surface))'
                      : hoveredArtistId === artist.id
                        ? 'color-mix(in srgb, var(--accent) 14%, var(--surface))'
                        : 'var(--surface)',
                    borderColor: selected
                      ? 'color-mix(in srgb, var(--accent) 34%, var(--border))'
                      : hoveredArtistId === artist.id
                        ? 'color-mix(in srgb, var(--accent) 34%, var(--border))'
                        : 'var(--border)',
                  }),
              ...(selectMode && !selectable ? { opacity: 0.5 } : {}),
            }}
            onClick={handleActivate}
            onMouseEnter={() => setHoveredArtistId(artist.id)}
            onMouseLeave={() => setHoveredArtistId((prev) => (prev === artist.id ? null : prev))}
            onKeyDown={(event) => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                handleActivate();
              }
            }}
            title={selectMode && !selectable ? 'Not eligible for merging' : artist.name}
          >
            <div style={{
              ...L.gridArt,
              ...(hybridPreview && hoveredArtistId === artist.id ? L.gridArtHovered : {}),
              ...(selected ? { outline: '2px solid var(--accent)', outlineOffset: -2 } : {}),
            }}>
              <ArtistTileImage artistId={artist.id} artist={artist.name} />
              {hybridPreview && (
                <div
                  data-hybrid-art-hover-overlay="artist"
                  aria-hidden="true"
                  style={{
                    ...L.gridArtHoverOverlay,
                    opacity: hoveredArtistId === artist.id ? 1 : 0,
                  }}
                />
              )}
              {selectMode ? (
                <div
                  aria-hidden="true"
                  style={{
                    position: 'absolute', top: 8, left: 8, zIndex: 2,
                    width: 20, height: 20, borderRadius: 5,
                    background: selected ? 'var(--accent)' : 'color-mix(in srgb, var(--surface) 72%, transparent)',
                    backdropFilter: 'blur(2px)',
                    border: `1.5px solid ${selected ? 'var(--accent)' : 'var(--border)'}`,
                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                    opacity: selectable ? 1 : 0.45,
                  }}
                >
                  {selected && (
                    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                  )}
                </div>
              ) : (
                <KebabButton
                  target={{ kind: 'artist', artistId: artist.id, name: artist.name }}
                  callbacks={{ onPlay: () => onPlay(artist), onOpen: () => onSelect(artist) }}
                  visible={hoveredArtistId === artist.id}
                  style={L.gridArtKebabBtn}
                />
              )}
            </div>
            <div style={L.gridTitle}>
              {artist.name}
              {!!artist.metadata_locked && <LockBadge />}
            </div>
          </div>
          );
        })}
      </div>
      {alphabeticalJump && (
        <QuickJumpRail availableLetters={availableLetters} activeLetter={activeLetter} onJump={jumpToLetter} />
      )}
    </div>
  );
}
// Root album grid layout constants (must track `L.gridWrap`/`L.gridTileBtn`
// below): minmax(160px,1fr) columns, 12px gap, 16px container padding on every
// side. Metadata height is the non-art part of a tile (padding + border +
// title + optional artist link + rating row), rounded up from the styled
// line-heights for safety headroom against font-rendering variance across
// platforms — see Phase 2 manual verification in the performance plan.
const GRID_MIN_TILE = 160;
const GRID_GAP = 12;
const GRID_PADDING_X = 16;
const GRID_METADATA_HEIGHT_WITH_ARTIST = 92;
const GRID_METADATA_HEIGHT_NO_ARTIST = 72;

/** Exported for direct windowing tests only (root Browse renders it internally). */
export function AlbumGrid({
  albums, loading, onSelect, onPlay, onQueue, onArtistSelect, showArtist = false, groupBy = 'artist', alphabeticalJump = false, initialScrollTop = 0, initialAnchorId, onScrollTopChange, onAnchorChange, hybridPreview = false,
}: {
  albums: Album[]; loading: boolean; onSelect: (a: Album) => void;
  onPlay: (a: Album) => void;
  onQueue: (a: Album) => void;
  onArtistSelect?: (artistName: string, album: Album) => void;
  showArtist?: boolean;
  groupBy?: 'artist' | 'album_artist';
  alphabeticalJump?: boolean;
  initialScrollTop?: number;
  initialAnchorId?: ClientEntityId;
  onScrollTopChange?: (top: number) => void;
  onAnchorChange?: (id: ClientEntityId) => void;
  hybridPreview?: boolean;
}) {
  const gridRef = useRef<HTMLDivElement | null>(null);
  const anchorRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const [activeLetter, setActiveLetter] = useState('#');
  const [hoveredAlbumId, setHoveredAlbumId] = useState<ClientEntityId | null>(null);
  const [hoveredArtistLinkId, setHoveredArtistLinkId] = useState<ClientEntityId | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [focusedIndex, setFocusedIndex] = useState<number | null>(null);
  const letterFirstIndexMap = useMemo(() => buildLetterFirstIndexMap(albums, (album) => album.title), [albums]);
  const availableLetters = useMemo(() => new Set(Object.keys(letterFirstIndexMap)), [letterFirstIndexMap]);

  // Windowing only activates once the container has a real measured width (see
  // useContainerSize) — unmeasured environments (tests, a hidden container at
  // mount) fall back to the original unwindowed render below.
  const { size: containerSize, attachRef: attachGridRef } = useContainerSize(gridRef);
  const windowed = containerSize.width > 0 || albums.length >= WINDOWING_SIZE_THRESHOLD;
  const metadataHeight = showArtist ? GRID_METADATA_HEIGHT_WITH_ARTIST : GRID_METADATA_HEIGHT_NO_ARTIST;
  const columns = windowed ? computeGridColumns(containerSize.width, GRID_MIN_TILE, GRID_GAP, GRID_PADDING_X) : 0;
  const tileWidth = windowed ? computeGridTileWidth(containerSize.width, columns, GRID_GAP, GRID_PADDING_X) : 0;
  const rowHeight = windowed ? tileWidth + metadataHeight + GRID_GAP : 0;
  const totalRows = windowed ? Math.ceil(albums.length / Math.max(1, columns)) : 0;
  const canvasHeight = windowed && totalRows > 0 ? totalRows * rowHeight - GRID_GAP : 0;

  // Initial scroll position restore — same DOM assignment either way; also
  // syncs the windowed-mode scrollTop state so the first windowed render
  // already reflects the restored position instead of waiting for a scroll event.
  useEffect(() => {
    const container = gridRef.current;
    if (!container) return;
    if (initialScrollTop > 0) {
      container.scrollTop = initialScrollTop;
      setScrollTop(initialScrollTop);
    }
  }, [initialScrollTop, albums.length]);

  // Initial anchor restore — unwindowed: DOM measurement (existing behavior).
  useEffect(() => {
    if (!initialAnchorId || windowed) return;
    const container = gridRef.current;
    if (!container) return;
    const node = container.querySelector<HTMLElement>(`[data-root-anchor-id="${initialAnchorId}"]`);
    if (!node) return;
    if (typeof node.scrollIntoView === 'function') {
      node.scrollIntoView({ block: 'nearest' });
    }
  }, [initialAnchorId, albums.length, windowed]);

  // Initial anchor restore — windowed: maps the album id to its current sorted
  // index, then to a row offset, with no DOM dependency (plan §7 item 3).
  useEffect(() => {
    if (!initialAnchorId || !windowed || columns <= 0 || rowHeight <= 0) return;
    const container = gridRef.current;
    if (!container) return;
    const idx = albums.findIndex((a) => a.id === initialAnchorId);
    if (idx < 0) return;
    const top = Math.floor(idx / columns) * rowHeight;
    container.scrollTop = top;
    setScrollTop(top);
  }, [initialAnchorId, albums, windowed, columns, rowHeight]);

  useEffect(() => {
    const container = gridRef.current;
    if (!container) return;
    const onScroll = () => onScrollTopChange?.(container.scrollTop);
    container.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      container.removeEventListener('scroll', onScroll);
    };
  }, [onScrollTopChange]);

  // Windowed-mode scroll tracking: keeps `scrollTop` state (which drives the
  // rendered row window below) in sync, rAF-throttled like the alpha-jump
  // active-letter tracker below.
  useEffect(() => {
    if (!windowed) return;
    const container = gridRef.current;
    if (!container) return;
    let rafId = 0;
    const onScroll = () => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        setScrollTop(container.scrollTop);
      });
    };
    container.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      container.removeEventListener('scroll', onScroll);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [windowed]);

  // Alpha-rail active-letter tracking — unwindowed: DOM offsetTop measurement
  // (existing behavior).
  useEffect(() => {
    if (!alphabeticalJump || windowed) return;
    const container = gridRef.current;
    if (!container) return;
    let rafId = 0;
    const updateActiveLetter = () => {
      const y = container.scrollTop + 2;
      let nextActive: string | null = null;
      for (const letter of ALPHA_RAIL_LETTERS) {
        const anchor = anchorRefs.current[letter];
        if (!anchor) continue;
        if (anchor.offsetTop <= y) nextActive = letter;
        else break;
      }
      if (!nextActive) {
        nextActive = ALPHA_RAIL_LETTERS.find((letter) => !!anchorRefs.current[letter]) ?? '#';
      }
      setActiveLetter((prev) => (prev === nextActive ? prev : nextActive!));
    };
    const onScroll = () => {
      onScrollTopChange?.(container.scrollTop);
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        updateActiveLetter();
      });
    };
    updateActiveLetter();
    container.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      container.removeEventListener('scroll', onScroll);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [alphabeticalJump, albums, onScrollTopChange, windowed]);

  // Alpha-rail active-letter tracking — windowed: derived from the row index
  // math instead of measuring DOM anchors that may not be mounted.
  useEffect(() => {
    if (!alphabeticalJump || !windowed || columns <= 0 || rowHeight <= 0) return;
    const currentRow = Math.floor(scrollTop / rowHeight);
    let nextActive: string | null = null;
    for (const letter of ALPHA_RAIL_LETTERS) {
      const idx = letterFirstIndexMap[letter];
      if (idx === undefined) continue;
      const row = Math.floor(idx / columns);
      if (row <= currentRow) nextActive = letter;
      else break;
    }
    if (!nextActive) {
      nextActive = ALPHA_RAIL_LETTERS.find((letter) => letterFirstIndexMap[letter] !== undefined) ?? '#';
    }
    setActiveLetter((prev) => (prev === nextActive ? prev : nextActive!));
  }, [alphabeticalJump, windowed, scrollTop, columns, rowHeight, letterFirstIndexMap]);

  const jumpToLetter = useCallback((letter: string) => {
    const container = gridRef.current;
    if (!container) return;
    if (windowed && columns > 0 && rowHeight > 0) {
      const idx = letterFirstIndexMap[letter];
      if (idx === undefined) return;
      const top = Math.floor(idx / columns) * rowHeight;
      container.scrollTo({ top, behavior: 'smooth' });
      return;
    }
    const anchor = anchorRefs.current[letter];
    if (!anchor) return;
    container.scrollTo({ top: anchor.offsetTop, behavior: 'smooth' });
  }, [windowed, columns, rowHeight, letterFirstIndexMap]);

  if (loading && !albums.length) return <div style={L.empty}>Loading...</div>;
  if (!albums.length) return <div style={L.empty}>No albums found.</div>;

  const renderTile = (album: Album, index: number, extraStyle: React.CSSProperties | undefined, attachAnchorRef: boolean) => {
    const displayArtist = getAlbumDisplayArtist(album, groupBy);
    return (
      <div
        role="button"
        tabIndex={0}
        key={album.id}
        ref={attachAnchorRef && letterFirstIndexMap[toAlphaBucket(album.title)] === index
          ? (el: HTMLDivElement | null) => { anchorRefs.current[toAlphaBucket(album.title)] = el; }
          : undefined}
        data-root-anchor-id={attachAnchorRef ? album.id : undefined}
        style={{
          ...L.gridTileBtn,
          ...extraStyle,
          ...(hybridPreview
            ? {
                backgroundColor: 'transparent',
                borderColor: 'transparent',
                boxShadow: 'none',
              }
            : {
                backgroundColor: hoveredAlbumId === album.id
                  ? 'color-mix(in srgb, var(--accent) 14%, var(--surface))'
                  : 'var(--surface)',
                borderColor: hoveredAlbumId === album.id
                  ? 'color-mix(in srgb, var(--accent) 34%, var(--border))'
                  : 'var(--border)',
              }),
        }}
        onClick={() => {
          const container = gridRef.current;
          if (container) onScrollTopChange?.(container.scrollTop);
          onAnchorChange?.(album.id);
          onSelect(album);
        }}
        onMouseEnter={() => setHoveredAlbumId(album.id)}
        onMouseLeave={() => setHoveredAlbumId((prev) => (prev === album.id ? null : prev))}
        onFocus={() => setFocusedIndex(index)}
        onBlur={() => setFocusedIndex((prev) => (prev === index ? null : prev))}
        onKeyDown={(event) => {
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            const container = gridRef.current;
            if (container) onScrollTopChange?.(container.scrollTop);
            onAnchorChange?.(album.id);
            onSelect(album);
          }
        }}
        title={album.title}
      >
        <div style={{
          ...L.gridArt,
          ...(hybridPreview && hoveredAlbumId === album.id ? L.gridArtHovered : {}),
        }}>
          <AlbumTileImage albumId={album.id} title={album.title} />
          {hybridPreview && (
            <div
              data-hybrid-art-hover-overlay="album"
              aria-hidden="true"
              style={{
                ...L.gridArtHoverOverlay,
                opacity: hoveredAlbumId === album.id ? 1 : 0,
              }}
            />
          )}
          <KebabButton
            target={{ kind: 'album', albumId: album.id, title: album.title }}
            callbacks={{ onPlay: () => onPlay(album), onQueue: () => onQueue(album) }}
            visible={hoveredAlbumId === album.id}
            style={L.gridArtKebabBtn}
          />
          <button
            type="button"
            style={{
              ...L.gridArtPlayBtn,
              opacity: hoveredAlbumId === album.id ? 1 : 0,
              pointerEvents: hoveredAlbumId === album.id ? 'auto' : 'none',
            }}
            title="Play album"
            aria-label={`Play album ${album.title}`}
            onClick={(e) => {
              e.stopPropagation();
              onPlay(album);
            }}
          >
            <PlayIcon size={14} />
          </button>
        </div>
        <div style={L.gridTitle}>{album.title}</div>
        {showArtist && displayArtist && (
          <button
            type="button"
            style={{
              ...L.gridArtistLink,
              color: hoveredArtistLinkId === album.id ? 'var(--accent)' : 'var(--text-muted)',
              textDecoration: hoveredArtistLinkId === album.id ? 'underline' : 'none',
              textUnderlineOffset: hoveredArtistLinkId === album.id ? 2 : undefined,
            }}
            onClick={(event) => {
              event.stopPropagation();
              onArtistSelect?.(displayArtist, album);
            }}
            onMouseEnter={() => setHoveredArtistLinkId(album.id)}
            onMouseLeave={() => setHoveredArtistLinkId((prev) => (prev === album.id ? null : prev))}
            onFocus={() => setHoveredArtistLinkId(album.id)}
            onBlur={() => setHoveredArtistLinkId((prev) => (prev === album.id ? null : prev))}
            title={`Open artist ${displayArtist}`}
            aria-label={`Open artist ${displayArtist}`}
          >
            {displayArtist}
          </button>
        )}
        <div style={L.gridRatingWrap}>
          <StarRating
            value={album.rating ?? null}
            ariaLabel={`${album.title} album rating`}
            size="compact"
            subdued={true}
            showValue={true}
          />
        </div>
      </div>
    );
  };

  let gridBody: React.ReactNode;
  if (windowed) {
    let [startRow, endRow] = computeVisibleRowRange(scrollTop, containerSize.height, rowHeight, totalRows, GRID_OVERSCAN_ROWS);
    // Keep the focused tile mounted even if scrolled outside the window, so
    // tab/keyboard focus is never dropped onto an unmounted card.
    if (focusedIndex != null && columns > 0) {
      const focusedRow = Math.floor(focusedIndex / columns);
      if (focusedRow < startRow) startRow = focusedRow;
      if (focusedRow > endRow) endRow = focusedRow;
    }
    const startIndex = Math.max(0, startRow * columns);
    const endIndex = Math.min(albums.length, (endRow + 1) * columns);
    const items: React.ReactNode[] = [];
    for (let i = startIndex; i < endIndex; i++) {
      const row = Math.floor(i / columns);
      const col = i % columns;
      items.push(renderTile(albums[i], i, {
        position: 'absolute',
        top: row * rowHeight,
        left: col * (tileWidth + GRID_GAP),
        width: tileWidth,
      }, false));
    }
    gridBody = <div style={{ position: 'relative', height: canvasHeight }}>{items}</div>;
  } else {
    gridBody = albums.map((album, i) => renderTile(album, i, undefined, true));
  }

  return (
    <div style={L.alphaShellFill}>
      <div
        ref={attachGridRef}
        style={{
          ...L.gridWrap,
          ...(alphabeticalJump ? L.alphaScrollable : {}),
          ...(windowed ? { display: 'block' } : {}),
        }}
      >
        {gridBody}
      </div>
      {alphabeticalJump && (
        <QuickJumpRail availableLetters={availableLetters} activeLetter={activeLetter} onJump={jumpToLetter} />
      )}
    </div>
  );
}
function LockBadge() {
  return (
    <span title="Custom metadata — protected from auto-scan" style={{ display: 'inline-flex', alignItems: 'center', gap: 4, fontSize: 11, color: 'var(--accent)', verticalAlign: 'middle', marginLeft: 8 }}>
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>
    </span>
  );
}

function ArtistHeader({
  artist, onPlayRadio, radioLoading, onRefreshed, onRateArtist, adaptiveAccentEnabled, canEditMetadata = false,
}: {
  artist: Artist;
  onPlayRadio: () => void;
  radioLoading: boolean;
  onRefreshed: () => void;
  onRateArtist: (rating: number | null) => void | Promise<void>;
  adaptiveAccentEnabled: boolean;
  canEditMetadata?: boolean;
}) {
  const [showMeta, setShowMeta] = useState(false);
  const [showEdit, setShowEdit] = useState(false);
  const [photoToken, setPhotoToken] = useState(0);
  const [mergeInfo, setMergeInfo] = useState<ArtistMergeInfo | null>(null);
  const [showUnmerge, setShowUnmerge] = useState(false);
  const [lockingIdentity, setLockingIdentity] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setMergeInfo(null);
    api.getArtistMergeInfo(artist.id).then((info) => {
      if (!cancelled) setMergeInfo(info);
    }).catch(() => {});
    return () => { cancelled = true; };
  }, [artist.id]);

  const handleLockNow = async () => {
    if (lockingIdentity) return;
    setLockingIdentity(true);
    try {
      await api.lockArtistIdentity(artist.id);
      onRefreshed();
    } catch {
      // Surfaced only via the state staying pending — not critical enough
      // to interrupt the page with an error banner.
    } finally {
      setLockingIdentity(false);
    }
  };

  return (
    <div style={L.artistHeader}>
      <div style={L.artistHeaderLeft}>
        <ArtistPhoto artistId={artist.id} artist={artist.name} refreshToken={photoToken} adaptiveAccentEnabled={adaptiveAccentEnabled} />
        <div>
          <div style={L.albumTitle}>
            {artist.name}
            {!!artist.metadata_locked && <LockBadge />}
          </div>
          <div style={L.albumMeta}>
            {artist.album_count} {artist.album_count === 1 ? 'album' : 'albums'} · {artist.track_count} tracks
          </div>
          {!!artist.identity_lock_pending && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, fontSize: 12, color: 'var(--text-muted)', marginTop: 6 }}>
              <span style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: 'var(--accent)', opacity: 0.6, animation: 'pulse 1.2s ease-in-out infinite' }} />
              <span>Matching artist metadata online…</span>
              {canEditMetadata && (
                <button
                  onClick={handleLockNow}
                  disabled={lockingIdentity}
                  style={{ background: 'none', border: 'none', padding: 0, color: 'var(--accent)', cursor: lockingIdentity ? 'default' : 'pointer', fontSize: 11, fontFamily: 'inherit' }}
                >
                  {lockingIdentity ? 'Locking…' : 'Lock now'}
                </button>
              )}
            </div>
          )}
          {!!mergeInfo?.merged && !!mergeInfo.members.length && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12, color: 'var(--text-muted)', marginTop: 6, flexWrap: 'wrap' }}>
              <span>
                Merged from: {mergeInfo.members.map((m, i) => (
                  <React.Fragment key={String(m.id)}>
                    {i > 0 ? ', ' : ''}<span style={{ color: 'var(--text)' }}>{m.original_name}</span>
                  </React.Fragment>
                ))}
              </span>
              {canEditMetadata && (
                <>
                  <span style={{ opacity: 0.4 }}>·</span>
                  <button
                    onClick={() => setShowUnmerge(true)}
                    style={{ background: 'none', border: 'none', padding: 0, color: 'var(--accent)', cursor: 'pointer', fontSize: 12, fontFamily: 'inherit' }}
                  >
                    Unmerge
                  </button>
                </>
              )}
            </div>
          )}
          {!!artist.styles?.length && (
            <div style={{ ...L.bioTags, marginTop: 8 }}>
              {artist.styles.map(style => (
                <span key={style} style={L.bioTag}>{style}</span>
              ))}
            </div>
          )}
          <div style={{ marginTop: 10 }}>
            <StarRating
              value={artist.rating ?? null}
              onChange={onRateArtist}
              ariaLabel={`${artist.name} artist rating`}
              showValue={true}
            />
          </div>
          <div style={{ display: 'flex', gap: 8, marginTop: 12, flexWrap: 'wrap' }}>
            <button
              style={{ ...L.btnPrimary, opacity: radioLoading ? 0.7 : 1 }}
              onClick={onPlayRadio}
              disabled={radioLoading}
              title="Build a random radio queue from similar style tags"
            >
              <PlayIcon /> {radioLoading ? 'Building Radio...' : 'Play Artist Radio'}
            </button>
            <button style={L.btnSecondary} onClick={() => setShowEdit(true)}>Edit</button>
            <button style={L.btnSecondary} onClick={() => setShowMeta(true)}>
              Refresh Metadata
            </button>
          </div>
        </div>
      </div>
      <LastFmBio artist={artist.name} />
      {showMeta && (
        <MetadataRefreshModal
          mode="artist"
          entityId={artist.id}
          initialArtist={artist.name}
          onClose={() => setShowMeta(false)}
          onApplied={() => { setShowMeta(false); onRefreshed(); }}
        />
      )}
      {showEdit && (
        <MetadataEditModal
          mode="artist"
          entityId={artist.id}
          initialData={artist}
          onClose={() => setShowEdit(false)}
          onSaved={() => { setShowEdit(false); setPhotoToken(t => t + 1); onRefreshed(); }}
        />
      )}
      {showUnmerge && mergeInfo?.merged && (
        <UnmergeModal
          artistId={artist.id}
          artistName={artist.name}
          members={mergeInfo.members}
          onClose={() => setShowUnmerge(false)}
          onUnmerged={() => { setShowUnmerge(false); onRefreshed(); }}
        />
      )}
    </div>
  );
}



function TrackList({
  tracks, loading, album, lastfmKey,
  onPlayTrack, onQueueTrack, onPlayAll, onQueueAll, onPlayVinyl, onRefreshed, onArtistClick, onRateAlbum, onRateTrack, adaptiveAccentEnabled,
  ratingFilter, onRatingFilterChange, trackSortMode, trackSortDir, onTrackSortModeChange, onTrackSortDirChange,
}: {
  tracks: Track[]; loading: boolean; album: Album; lastfmKey: string;
  onPlayTrack: (t: Track) => void;
  onQueueTrack: (t: Track) => void;
  onPlayAll: () => void;
  onQueueAll: () => void;
  onPlayVinyl: () => void;
  onRefreshed: (mergedIntoId?: ClientEntityId) => void;
  onArtistClick?: () => void;
  onRateAlbum: (rating: number | null) => void | Promise<void>;
  onRateTrack: (track: Track, rating: number | null) => void | Promise<void>;
  adaptiveAccentEnabled: boolean;
  ratingFilter: RatingFilter;
  onRatingFilterChange: (filter: RatingFilter) => void;
  trackSortMode: TrackSortMode;
  trackSortDir: 'asc' | 'desc';
  onTrackSortModeChange: (mode: TrackSortMode) => void;
  onTrackSortDirChange: (dir: 'asc' | 'desc') => void;
}) {
  const [showMeta, setShowMeta] = useState(false);
  const [showEdit, setShowEdit] = useState(false);
  const [coverRefreshToken, setCoverRefreshToken] = useState(0);
  const [hoveredTrackId, setHoveredTrackId] = useState<ClientEntityId | null>(null);
  if (loading) return <div style={L.empty}>Loading…</div>;
  if (!tracks.length) return <div style={L.empty}>No tracks found.</div>;

  const totalDur = tracks.reduce((acc, t) => acc + (t.duration ?? 0), 0);
  const artistName = album.album_artist || album.artist || '';

  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: 0, flex: 1 }}>
      {/* Album header: cover+meta on left, Last.fm bio on right */}
      <div style={L.albumHeader}>
        {/* Left pane: art + title + buttons */}
        <div style={{ display: 'flex', alignItems: 'flex-start', gap: 20, flexShrink: 0 }}>
          <AlbumCover albumId={album.id} title={album.title} refreshToken={coverRefreshToken} adaptiveAccentEnabled={adaptiveAccentEnabled} />
          <div style={{ minWidth: 0 }}>
            <div style={L.albumTitle}>{album.title}{!!album.metadata_locked && <LockBadge />}</div>
            <div style={L.albumRatingRow}>
              <StarRating
                value={album.rating ?? null}
                onChange={onRateAlbum}
                ariaLabel={`${album.title} album rating`}
                size="hero"
                showValue={true}
              />
            </div>
            <div style={L.albumMeta}>
              {(() => {
                const displayArtist = album.album_artist && album.album_artist !== album.artist
                  ? `${album.album_artist} (Album Artist)` : album.artist;
                const rest = [album.year, album.label, `${tracks.length} tracks`, fmtDur(totalDur)].filter(Boolean).join(' · ');
                return (<>
                  {displayArtist && onArtistClick
                    ? <button onClick={onArtistClick} style={{ background: 'none', border: 'none', padding: 0, color: 'var(--accent)', cursor: 'pointer', fontSize: 'inherit', fontFamily: 'inherit', textDecoration: 'none' }} onMouseEnter={e => (e.currentTarget.style.textDecoration = 'underline')} onMouseLeave={e => (e.currentTarget.style.textDecoration = 'none')}>{displayArtist}</button>
                    : displayArtist}
                  {displayArtist && rest ? ' · ' : ''}{rest}
                </>);
              })()}
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, auto)', gap: '6px 8px', marginTop: 14, justifyContent: 'start' }}>
              <button style={L.btnPrimary} onClick={onPlayAll}>
                <PlayIcon /> Play All
              </button>
              <button style={L.btnSecondary} onClick={onQueueAll}>+ Queue All</button>
              <button style={L.btnSecondary} onClick={onPlayVinyl}>Vinyl Mode</button>
              <button style={L.btnSecondary} onClick={() => setShowEdit(true)}>Edit</button>
              <button style={L.btnSecondary} onClick={() => setShowMeta(true)}>Refresh Metadata</button>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginTop: 14, flexWrap: 'wrap' }}>
              <div style={L.toggleWrap} title="Sort album tracks">
                <span style={L.toggleLabel}>Track Sort</span>
                <div style={L.togglePill}>
                  <button
                    style={{ ...L.toggleOpt, ...(trackSortMode === 'album' ? L.toggleOptActive : {}) }}
                    onClick={() => onTrackSortModeChange('album')}
                  >
                    Album Order
                  </button>
                  <button
                    style={{ ...L.toggleOpt, ...(trackSortMode === 'rating' ? L.toggleOptActive : {}) }}
                    onClick={() => {
                      if (trackSortMode === 'rating') onTrackSortDirChange(trackSortDir === 'asc' ? 'desc' : 'asc');
                      else onTrackSortModeChange('rating');
                    }}
                  >
                    Rating{trackSortMode === 'rating' ? (trackSortDir === 'asc' ? ' ↑' : ' ↓') : ''}
                  </button>
                </div>
              </div>
              <div style={L.toggleWrap} title="Filter album tracks by rating">
                <span style={L.toggleLabel}>Track Rating</span>
                <div style={L.togglePill}>
                  {([
                    ['all', 'All'],
                    ['rated', 'Rated'],
                    ['unrated', 'Unrated'],
                    ['gte4', '4+'],
                    ['gte3', '3+'],
                  ] as Array<[RatingFilter, string]>).map(([value, label]) => (
                    <button
                      key={value}
                      style={{ ...L.toggleOpt, ...(ratingFilter === value ? L.toggleOptActive : {}) }}
                      onClick={() => onRatingFilterChange(value)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>
            </div>
          </div>
        </div>
        {/* Right pane: Last.fm review */}
        <LastFmBio artist={artistName} album={album.title} />
      </div>
      {showMeta && (
        <MetadataRefreshModal
          mode="album"
          entityId={album.id}
          initialArtist={artistName}
          initialAlbum={album.title}
          onClose={() => setShowMeta(false)}
          onApplied={(mergedIntoId) => { setShowMeta(false); setCoverRefreshToken((v) => v + 1); onRefreshed(mergedIntoId); }}
        />
      )}
      {showEdit && (
        <MetadataEditModal
          mode="album"
          entityId={album.id}
          initialData={album}
          onClose={() => setShowEdit(false)}
          onSaved={() => { setShowEdit(false); setCoverRefreshToken((v) => v + 1); onRefreshed(); }}
        />
      )}

      {/* Track rows */}
      <div style={{ flex: 1, overflowY: 'auto', paddingBottom: 0 }}>
        {tracks.map((track, i) => (
          <div
            key={track.id}
            style={{
              ...L.trackRow,
              backgroundColor: hoveredTrackId === track.id
                ? 'color-mix(in srgb, var(--accent) 14%, transparent)'
                : i % 2 === 0 ? 'transparent' : 'rgba(255,255,255,0.018)',
            }}
            onClick={() => onPlayTrack(track)}
            onMouseEnter={() => setHoveredTrackId(track.id)}
            onMouseLeave={() => setHoveredTrackId((prev) => (prev === track.id ? null : prev))}
          >
            <div style={L.trackNum}>
              {track.track_number ?? <span style={{ opacity: 0.3 }}>–</span>}
            </div>
            <button
              style={L.playRowBtn}
              onClick={e => { e.stopPropagation(); onPlayTrack(track); }}
              title="Play"
            >
              <PlayIcon />
            </button>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={L.trackTitle}>{track.title || track.file_name}</div>
              {track.artist && track.artist !== album.artist && (
                <div style={L.secondaryText}>{track.artist}</div>
              )}
            </div>
            {track.bitrate && (
              <div style={L.meta}>{track.bitrate}k</div>
            )}
            <div
              style={L.trackRatingCell}
              onClick={(event) => event.stopPropagation()}
              onMouseDown={(event) => event.stopPropagation()}
            >
              <StarRating
                value={track.rating ?? null}
                onChange={(rating) => onRateTrack(track, rating)}
                ariaLabel={`${track.title || track.file_name} track rating`}
                size="compact"
                subdued={hoveredTrackId !== track.id && !track.rating}
              />
            </div>
            {track.has_deep_analysis && (
              <span style={{ fontSize: 10, color: 'var(--accent)', opacity: 0.55, flexShrink: 0 }} title="Sonic Fingerprint available — AI stem analysis complete">✦</span>
            )}
            <div style={{ ...L.meta, minWidth: 44, textAlign: 'right', fontVariantNumeric: 'tabular-nums' }}>
              {fmtTrackDur(track.duration)}
            </div>
            <KebabButton
              target={{ kind: 'track', trackId: track.id, title: track.title || track.file_name }}
              callbacks={{ onPlay: () => onPlayTrack(track), onQueue: () => onQueueTrack(track) }}
              visible={hoveredTrackId === track.id}
            />
          </div>
        ))}
      </div>
    </div>
  );
}

// ─── Browse View (top-level) ──────────────────────────────────────────────────

type BrowseTab = 'artists' | 'albums';
type RootViewMode = 'table' | 'grid';
type DrillState =
  | { level: 'root' }
  | { level: 'artist'; artist: Artist }
  | { level: 'album'; album: Album; artist: Artist | null };

interface Props {
  libraries: Library[];
  playTrack: (track: Track, allTracks?: Track[], source?: import('../types').QueueSource) => void;
  playAlbumInVinylMode: (tracks: Track[], albumId: ClientEntityId) => void;
  addToQueue: (track: Track) => void;
  lastfmKey: string;
  openAlbumRequest?: { album: Album; token: number } | null;
  openArtistRequest?: { artist: Artist; token: number } | null;
  openGenreRequest?: { genre: string; token: number } | null;
  resetRequest?: number | null;
  adaptiveAccentEnabled?: boolean;
  forcedLibraryIds?: ClientEntityId[] | null;
  hybridPreview?: boolean;
  hideCompilationOnlyArtists?: boolean;
  /** Admins, and users granted "Allow metadata editing", may merge/unmerge
   * duplicate artists (artist consolidation). Mirrors the server's
   * `can_edit_metadata` check — see `music_routes.rs`. */
  canEditMetadata?: boolean;
}

/** Browse View is part of this module's public API. */
export default function BrowseView({
  libraries,
  playTrack,
  playAlbumInVinylMode,
  addToQueue,
  lastfmKey,
  openAlbumRequest,
  openArtistRequest,
  openGenreRequest,
  resetRequest,
  adaptiveAccentEnabled = true,
  forcedLibraryIds = null,
  hybridPreview = false,
  hideCompilationOnlyArtists = true,
  canEditMetadata = false,
}: Props) {
  const [tab, setTab]         = useState<BrowseTab>('artists');
  const [rootViewMode, setRootViewMode] = useState<RootViewMode>(() =>
    (safeLocalStorageGet('browse_root_view_mode') as RootViewMode) || 'grid'
  );
  const setPersistedRootViewMode = useCallback((mode: RootViewMode) => {
    safeLocalStorageSet('browse_root_view_mode', mode);
    setRootViewMode(mode);
  }, []);
  // Artist consolidation multi-select (see wip/artist-consolidation-implementation-plan.md).
  const [artistSelectMode, setArtistSelectMode] = useState(false);
  const [selectedArtistIds, setSelectedArtistIds] = useState<Set<ClientEntityId>>(new Set());
  const [showMergeModal, setShowMergeModal] = useState(false);
  const toggleArtistSelected = useCallback((artist: Artist) => {
    setSelectedArtistIds((prev) => {
      const next = new Set(prev);
      if (next.has(artist.id)) next.delete(artist.id); else next.add(artist.id);
      return next;
    });
  }, []);
  const exitArtistSelectMode = useCallback(() => {
    setArtistSelectMode(false);
    setSelectedArtistIds(new Set());
  }, []);
  const [groupBy, setGroupBy] = useState<'artist' | 'album_artist'>('album_artist');
  const [drill, setDrill]     = useState<DrillState>({ level: 'root' });
  const [genreOptions, setGenreOptions] = useState<Genre[]>([]);
  const [selectedGenres, setSelectedGenres] = useState<string[]>([]);
  const [selectedLibraryIds, setSelectedLibraryIds] = useState<ClientEntityId[]>([]);
  const [genreFilterOpen, setGenreFilterOpen] = useState(false);
  const [libraryFilterOpen, setLibraryFilterOpen] = useState(false);
  const [refineOpen, setRefineOpen] = useState(false);
  const genreFilterRef = useRef<HTMLDivElement | null>(null);
  const libraryFilterRef = useRef<HTMLDivElement | null>(null);
  const refinePanelRef = useRef<HTMLDivElement | null>(null);
  const rootFetchTokenRef = useRef(0);
  const albumChangeCursorRef = useRef<number | null>(null);
  // Guards drill-down (artist/album) fetches the same way rootFetchTokenRef guards the
  // root lists: a slow background refresh must not overwrite a newer navigation's data.
  const drillFetchTokenRef = useRef(0);
  const releaseTypeResolvedArtistRef = useRef<Set<ClientEntityId>>(new Set());
  // Artist sort — persisted across sessions
  const [artistSortDir, _setArtistSortDir] = useState<'asc' | 'desc'>(() =>
    parseSortDir(safeLocalStorageGet('browse_artist_sort_dir'))
  );
  const setArtistSortDir = useCallback((d: 'asc' | 'desc') => {
    safeLocalStorageSet('browse_artist_sort_dir', d);
    _setArtistSortDir(d);
  }, []);

  // Album sort — persisted across sessions
  const [albumSortField, _setAlbumSortField] = useState<'title' | 'year' | 'rating'>(() =>
    (safeLocalStorageGet('browse_album_sort_field') as 'title' | 'year' | 'rating') || 'title'
  );
  const [albumSortDir, _setAlbumSortDir] = useState<'asc' | 'desc'>(() =>
    parseSortDir(safeLocalStorageGet('browse_album_sort_dir'))
  );
  const setAlbumSortField = useCallback((f: 'title' | 'year' | 'rating') => {
    safeLocalStorageSet('browse_album_sort_field', f);
    _setAlbumSortField(f);
  }, []);
  const setAlbumSortDir = useCallback((d: 'asc' | 'desc') => {
    safeLocalStorageSet('browse_album_sort_dir', d);
    _setAlbumSortDir(d);
  }, []);
  const [artists, setArtists] = useState<Artist[]>([]);
  const [allAlbums, setAllAlbums] = useState<Album[]>([]);
  const [artistAlbums, setArtistAlbums] = useState<Album[]>([]);
  const [appearsOnAlbums, setAppearsOnAlbums] = useState<Album[]>([]);
  const [albumTracks, setAlbumTracks]   = useState<Track[]>([]);
  const [sonicFingerprintOnly, setSonicFingerprintOnly] = useState(false);
  const [artistRatingFilter, setArtistRatingFilter] = useState<RatingFilter>('all');
  const [albumRatingFilter, setAlbumRatingFilter] = useState<RatingFilter>('all');
  const [trackRatingFilter, setTrackRatingFilter] = useState<RatingFilter>('all');
  const [trackSortMode, setTrackSortMode] = useState<TrackSortMode>('album');
  const [trackSortDir, setTrackSortDir] = useState<'asc' | 'desc'>('desc');
  const [loading, setLoading] = useState(false);
  // Metadata refresh overrides — updated in-place without touching drill or re-fetching tracks
  const [currentAlbum, setCurrentAlbum] = useState<Album | null>(null);
  const [currentArtist, setCurrentArtist] = useState<Artist | null>(null);
  const [artistRadioLoading, setArtistRadioLoading] = useState(false);
  const [topTracksPlayLoading, setTopTracksPlayLoading] = useState(false);
  const isLibraryScopeForced = Boolean(forcedLibraryIds && forcedLibraryIds.length > 0);
  const forcedLibraryScopeKey = (forcedLibraryIds ?? []).join(',');
  const activeLibraryIds = useMemo(
    () => selectedLibraryIds.length ? selectedLibraryIds : undefined,
    [selectedLibraryIds],
  );

  // Sorted artist list (client-side, derived from artists)
  const filteredArtists = useMemo(
    () => filterArtistsByRating(artists, artistRatingFilter),
    [artists, artistRatingFilter],
  );
  const sortedArtists = useMemo(
    () => sortArtists(filteredArtists, artistSortDir),
    [filteredArtists, artistSortDir],
  );
  const selectedArtists = useMemo(
    () => sortedArtists.filter((a) => selectedArtistIds.has(a.id)),
    [sortedArtists, selectedArtistIds],
  );

  // Sorted album list (client-side, derived from allAlbums)
  const filteredAlbums = useMemo(
    () => filterAlbumsByRating(allAlbums, albumRatingFilter),
    [allAlbums, albumRatingFilter],
  );
  const filteredArtistAlbums = useMemo(
    () => filterAlbumsByRating(artistAlbums, albumRatingFilter),
    [artistAlbums, albumRatingFilter],
  );
  const filteredAppearsOnAlbums = useMemo(
    () => filterAlbumsByRating(appearsOnAlbums, albumRatingFilter),
    [appearsOnAlbums, albumRatingFilter],
  );
  const sortedAlbums = useMemo(
    () => sortAlbums(filteredAlbums, albumSortField, albumSortDir),
    [filteredAlbums, albumSortField, albumSortDir],
  );
  const sortedArtistAlbums = useMemo(
    () => sortAlbums(filteredArtistAlbums, albumSortField, albumSortDir),
    [filteredArtistAlbums, albumSortField, albumSortDir],
  );
  const sortedAppearsOnAlbums = useMemo(
    () => sortAlbums(filteredAppearsOnAlbums, albumSortField, albumSortDir),
    [filteredAppearsOnAlbums, albumSortField, albumSortDir],
  );
  const displayedAlbumTracks = useMemo(
    () => sortTracks(filterTracksByRating(albumTracks, trackRatingFilter), trackSortMode, trackSortDir),
    [albumTracks, trackRatingFilter, trackSortMode, trackSortDir],
  );
  const artistAlbumSections = useMemo(
    () => groupArtistDiscographyByReleaseType(sortedArtistAlbums),
    [sortedArtistAlbums],
  );

  useEffect(() => {
    api.genres().then(setGenreOptions).catch(() => setGenreOptions([]));
  }, []);

  useEffect(() => {
    if (!forcedLibraryIds || forcedLibraryIds.length === 0) {
      setSelectedLibraryIds([]);
      setLibraryFilterOpen(false);
      return;
    }
    setSelectedLibraryIds((prev) => {
      if (prev.length === forcedLibraryIds.length && prev.every((id, index) => id === forcedLibraryIds[index])) {
        return prev;
      }
      return [...forcedLibraryIds];
    });
    setLibraryFilterOpen(false);
  }, [forcedLibraryIds, forcedLibraryScopeKey]);

  // Load root-level data. `showLoading` is false for background refreshes (e.g. while a
  // library scan keeps adding artists/albums) so the list updates in place instead of
  // flashing skeletons and losing the user's scroll position.
  const loadRootData = useCallback((showLoading: boolean) => {
    if (drill.level !== 'root') return;
    const fetchToken = ++rootFetchTokenRef.current;
    if (showLoading) setLoading(true);
    if (tab === 'artists') {
      api.artists({ genres: selectedGenres, library_ids: activeLibraryIds, sonic_fingerprint_only: sonicFingerprintOnly || undefined, hide_compilation_only: hideCompilationOnlyArtists })
        .then((rows) => {
          if (!shouldApplyBrowseRootFetchResult(fetchToken, rootFetchTokenRef.current)) return;
          setArtists(rows);
        })
        .catch(() => {})
        .finally(() => {
          if (!shouldApplyBrowseRootFetchResult(fetchToken, rootFetchTokenRef.current)) return;
          setLoading(false);
        });
    } else {
      return (async () => {
        let cursor: number | null = null;
        try {
          cursor = (await api.albumChangeCursor()).cursor;
        } catch {}
        try {
          const rows = await api.albums({ group_by: groupBy, genres: selectedGenres, library_ids: activeLibraryIds, sonic_fingerprint_only: sonicFingerprintOnly || undefined });
          if (!shouldApplyBrowseRootFetchResult(fetchToken, rootFetchTokenRef.current)) return;
          albumChangeCursorRef.current = cursor;
          setAllAlbums(rows);
        } catch {
          // Keep the last successful collection visible across transient failures.
        } finally {
          if (shouldApplyBrowseRootFetchResult(fetchToken, rootFetchTokenRef.current)) {
            setLoading(false);
          }
        }
      })();
    }
  }, [activeLibraryIds, drill.level, tab, groupBy, selectedGenres, sonicFingerprintOnly, hideCompilationOnlyArtists]);

  const loadRootAlbumChanges = useCallback(async () => {
    if (drill.level !== 'root' || tab !== 'albums') return;
    const after = albumChangeCursorRef.current;
    if (after == null) {
      await loadRootData(false);
      return;
    }
    const fetchToken = rootFetchTokenRef.current;
    try {
      const through = (await api.albumChangeCursor()).cursor;
      if (through <= after) return;
      const rows = await api.albums({
        group_by: groupBy,
        genres: selectedGenres,
        library_ids: activeLibraryIds,
        sonic_fingerprint_only: sonicFingerprintOnly || undefined,
        after_album_rowid: after,
        through_album_rowid: through,
      });
      if (!shouldApplyBrowseRootFetchResult(fetchToken, rootFetchTokenRef.current)) return;
      albumChangeCursorRef.current = through;
      if (rows.length) {
        setAllAlbums((current) => mergeAlbumChanges(current, rows, groupBy));
      }
    } catch {
      // Do not advance the cursor after a failed delta; the next poll retries the same range.
    }
  }, [activeLibraryIds, drill.level, groupBy, loadRootData, selectedGenres, sonicFingerprintOnly, tab]);

  useEffect(() => {
    loadRootData(true);
  }, [loadRootData]);

  // Load artist's albums and "appears on" compilations when drilling into artist
  const loadArtistData = useCallback((showLoading: boolean) => {
    if (drill.level !== 'artist') return;
    const fetchToken = ++drillFetchTokenRef.current;
    if (showLoading) {
      setLoading(true);
      setAppearsOnAlbums([]);
    }
    const artistAlbumsPromise = activeLibraryIds
      ? api.artistAlbums(drill.artist.id, activeLibraryIds)
      : api.artistAlbums(drill.artist.id);
    const artistAppearsOnPromise = activeLibraryIds
      ? api.artistAppearsOn(drill.artist.id, activeLibraryIds)
      : api.artistAppearsOn(drill.artist.id);
    Promise.all([
      artistAlbumsPromise,
      artistAppearsOnPromise,
    ]).then(([albums, appearsOn]) => {
      if (fetchToken !== drillFetchTokenRef.current) return;
      setArtistAlbums(albums);
      setAppearsOnAlbums(appearsOn);
    }).catch(() => {}).finally(() => {
      if (fetchToken !== drillFetchTokenRef.current) return;
      setLoading(false);
    });
  }, [drill, activeLibraryIds]);

  useEffect(() => {
    loadArtistData(true);
  }, [loadArtistData]);

  useEffect(() => {
    if (drill.level !== 'artist') return;
    const artistId = drill.artist.id;
    if (releaseTypeResolvedArtistRef.current.has(artistId)) return;
    if (typeof (api as any).resolveArtistReleaseTypes !== 'function') return;
    const hasResolvable = artistAlbums.some((album) => !album.metadata_locked && (!album.releaseType || album.releaseType === 'album'));
    if (!hasResolvable) return;
    releaseTypeResolvedArtistRef.current.add(artistId);
    api.resolveArtistReleaseTypes(artistId)
      .then((result) => {
        if (!result.updated) return;
        const refreshAlbums = activeLibraryIds
          ? api.artistAlbums(artistId, activeLibraryIds)
          : api.artistAlbums(artistId);
        refreshAlbums.then(setArtistAlbums).catch(() => {});
      })
      .catch(() => {});
  }, [drill, artistAlbums, activeLibraryIds]);

  // Helper: fetch tracks for an album, merging across split rows when in album_artist mode
  const fetchAlbumTracks = useCallback(async (album: Album, mode: 'artist' | 'album_artist') => {
    if (mode === 'album_artist') {
      return activeLibraryIds
        ? api.albumTracksByGroup(album.title, album.album_artist, activeLibraryIds)
        : api.albumTracksByGroup(album.title, album.album_artist);
    }
    return activeLibraryIds ? api.albumTracks(album.id, activeLibraryIds) : api.albumTracks(album.id);
  }, [activeLibraryIds]);

  // Load album tracks when drilling into album
  const loadAlbumTracks = useCallback((showLoading: boolean) => {
    if (drill.level !== 'album') return;
    const fetchToken = ++drillFetchTokenRef.current;
    if (showLoading) setLoading(true);
    // Use groupBy at the time of drilling — preserved in the closure via drill state
    const mode = drill.level === 'album' ? (drill as any)._groupBy ?? groupBy : groupBy;
    fetchAlbumTracks(drill.album, mode)
      .then((tracks) => {
        if (fetchToken !== drillFetchTokenRef.current) return;
        setAlbumTracks(tracks);
      })
      .catch(() => {})
      .finally(() => {
        if (fetchToken !== drillFetchTokenRef.current) return;
        setLoading(false);
      });
  }, [drill, fetchAlbumTracks, groupBy]);

  useEffect(() => {
    loadAlbumTracks(true);
  }, [loadAlbumTracks]);

  // Keep whichever level the user is looking at current while a library scan is running.
  useScanActivityRefresh(useCallback(({ scanActive, scanFinished }) => {
    if (drill.level === 'root' && tab === 'albums') {
      if (scanFinished) return loadRootData(false);
      if (scanActive) return loadRootAlbumChanges();
      return;
    }
    if (drill.level === 'root') loadRootData(false);
    else if (drill.level === 'artist') loadArtistData(false);
    else loadAlbumTracks(false);
  }, [drill.level, tab, loadRootData, loadRootAlbumChanges, loadArtistData, loadAlbumTracks]));

  const goRoot   = useCallback(() => { setCurrentArtist(null); setCurrentAlbum(null); setDrill({ level: 'root' }); }, []);
  const goArtist = useCallback((artist: Artist) => { setCurrentArtist(artist); setCurrentAlbum(null); setDrill({ level: 'artist', artist }); }, []);
  const goArtistByName = useCallback((artistName: string) => {
    const normalizedName = artistName.trim().toLowerCase();
    if (!normalizedName) return;
    const existingArtist = artists.find((artist) => artist.name.trim().toLowerCase() === normalizedName);
    if (existingArtist) {
      goArtist(existingArtist);
      return;
    }
    api.artists({ library_ids: activeLibraryIds })
      .then((allArtists) => {
        const matchedArtist = allArtists.find((artist) => artist.name.trim().toLowerCase() === normalizedName);
        if (matchedArtist) goArtist(matchedArtist);
      })
      .catch(() => {});
  }, [activeLibraryIds, artists, goArtist]);
  const goAlbum  = useCallback((album: Album, artist: Artist | null = null, mode?: 'artist' | 'album_artist') => {
    setCurrentArtist(null);
    setCurrentAlbum(album);
    const d: any = { level: 'album', album, artist, _groupBy: mode };
    setDrill(d);
  }, []);

  const playAlbum = useCallback(async (album: Album) => {
    const tracks = await fetchAlbumTracks(album, groupBy);
    if (tracks.length) {
      playTrack(tracks[0], tracks, { type: 'album', id: album.id });
    }
  }, [playTrack, fetchAlbumTracks, groupBy]);

  const queueAlbum = useCallback(async (album: Album) => {
    const tracks = await fetchAlbumTracks(album, groupBy);
    tracks.forEach(t => addToQueue(t));
  }, [addToQueue, fetchAlbumTracks, groupBy]);

  const updateAlbumRatingLocally = useCallback((target: Album, rating: number | null) => {
    setAllAlbums((prev) => applyAlbumRating(prev, target, rating));
    setArtistAlbums((prev) => applyAlbumRating(prev, target, rating));
    setAppearsOnAlbums((prev) => applyAlbumRating(prev, target, rating));
    setCurrentAlbum((prev) => (prev && matchesAlbumRatingTarget(prev, target) ? { ...prev, rating } : prev));
  }, []);

  const updateTrackRatingLocally = useCallback((trackId: ClientEntityId, rating: number | null) => {
    setAlbumTracks((prev) => applyTrackRating(prev, trackId, rating));
  }, []);

  const updateArtistRatingLocally = useCallback((artistId: ClientEntityId, rating: number | null) => {
    setArtists((prev) => applyArtistRating(prev, artistId, rating));
    setCurrentArtist((prev) => (prev && prev.id === artistId ? { ...prev, rating } : prev));
  }, []);

  const handleAlbumRatingChange = useCallback(async (target: Album, rating: number | null) => {
    const response = await api.setAlbumRating(target.id, rating);
    updateAlbumRatingLocally(target, response.rating);
  }, [updateAlbumRatingLocally]);

  const handleTrackRatingChange = useCallback(async (track: Track, rating: number | null) => {
    const response = await api.setTrackRating(track.id, rating);
    updateTrackRatingLocally(track.id, response.rating);
  }, [updateTrackRatingLocally]);

  const handleArtistRatingChange = useCallback(async (target: Artist, rating: number | null) => {
    const response = await api.setArtistRating(target.id, rating);
    updateArtistRatingLocally(target.id, response.rating);
  }, [updateArtistRatingLocally]);

  const toggleGenreFilterSelection = useCallback((genre: string) => {
    setSelectedGenres((prev) => toggleGenreSelection(prev, genre));
  }, []);

  const clearGenreFilter = useCallback(() => {
    setSelectedGenres([]);
  }, []);

  const toggleLibraryFilterSelection = useCallback((libraryId: ClientEntityId) => {
    if (isLibraryScopeForced) return;
    setSelectedLibraryIds((prev) => toggleLibrarySelection(prev, libraryId));
  }, [isLibraryScopeForced]);

  const clearLibraryFilter = useCallback(() => {
    if (isLibraryScopeForced) return;
    setSelectedLibraryIds([]);
  }, [isLibraryScopeForced]);

  const clearBrowseRefinements = useCallback(() => {
    setSelectedGenres([]);
    setGenreFilterOpen(false);
    setSonicFingerprintOnly(false);
    if (tab === 'artists') {
      setArtistRatingFilter('all');
      setArtistSortDir('asc');
    } else {
      setAlbumRatingFilter('all');
      setAlbumSortField('title');
      setAlbumSortDir('asc');
      setGroupBy('album_artist');
    }
    setPersistedRootViewMode('grid');
  }, [setAlbumSortDir, setAlbumSortField, setArtistSortDir, setPersistedRootViewMode, tab]);

  const rootScrollViewKey = `${tab}:${rootViewMode}`;
  const rootInitialScrollTop = ROOT_SCROLL_BY_VIEW[rootScrollViewKey] ?? 0;
  const rootInitialAnchorId = ROOT_ANCHOR_BY_VIEW[rootScrollViewKey];
  const handleRootScrollTopChange = useCallback((top: number) => {
    ROOT_SCROLL_BY_VIEW[rootScrollViewKey] = top;
  }, [rootScrollViewKey]);
  const handleRootAnchorChange = useCallback((id: ClientEntityId) => {
    ROOT_ANCHOR_BY_VIEW[rootScrollViewKey] = id;
  }, [rootScrollViewKey]);

  useEffect(() => {
    if (!genreFilterOpen && !libraryFilterOpen && !refineOpen) return;
    const handleMouseDown = (event: MouseEvent) => {
      if (genreFilterRef.current && !genreFilterRef.current.contains(event.target as Node)) {
        setGenreFilterOpen(false);
      }
      if (libraryFilterRef.current && !libraryFilterRef.current.contains(event.target as Node)) {
        setLibraryFilterOpen(false);
      }
      if (refinePanelRef.current && !refinePanelRef.current.contains(event.target as Node)) {
        setRefineOpen(false);
      }
    };
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setGenreFilterOpen(false);
        setLibraryFilterOpen(false);
        setRefineOpen(false);
      }
    };
    document.addEventListener('mousedown', handleMouseDown);
    document.addEventListener('keydown', handleEscape);
    return () => {
      document.removeEventListener('mousedown', handleMouseDown);
      document.removeEventListener('keydown', handleEscape);
    };
  }, [genreFilterOpen, libraryFilterOpen, refineOpen]);

  const playArtistRadio = useCallback(async (artist: Artist) => {
    setArtistRadioLoading(true);
    try {
      const result = await api.artistRadio(artist.id, 120);
      if (!result.tracks.length) {
        window.alert(`No radio tracks found for "${artist.name}".`);
        return;
      }
      playTrack(result.tracks[0], result.tracks);
    } catch (e: any) {
      window.alert(e.message || 'Failed to build artist radio.');
    } finally {
      setArtistRadioLoading(false);
    }
  }, [playTrack]);

  const playTopTracks = useCallback(async (artist: Artist, topTracks: LastFmTopTrack[]) => {
    if (!topTracks.length) return;
    setTopTracksPlayLoading(true);
    try {
      const resolved = (await Promise.all(topTracks.map(async (topTrack) => {
        const searchResult = await api.search({
          q: topTrack.name,
          limit: 200,
          sort: 'title',
          order: 'asc',
        });
        return resolveTopTrackFromLibrarySearch(artist.name, topTrack.name, searchResult.tracks);
      })))
        .filter((track): track is Track => Boolean(track))
        .filter((track, idx, arr) => arr.findIndex((t) => t.id === track.id) === idx);

      if (!resolved.length) {
        window.alert(`No matching tracks found in your library for "${artist.name}" top songs.`);
        return;
      }

      playTrack(resolved[0], resolved);
    } catch (e: any) {
      window.alert(e.message || 'Failed to build Top 5 playback queue.');
    } finally {
      setTopTracksPlayLoading(false);
    }
  }, [playTrack]);

  // Allow external navigation (Home tiles / Search) to open an album directly.
  useEffect(() => {
    if (!openAlbumRequest) return;
    setTab('albums');
    setCurrentAlbum(openAlbumRequest.album);
    const d: any = {
      level: 'album',
      album: openAlbumRequest.album,
      artist: null,
      _groupBy: 'album_artist',
    };
    setDrill(d);
  }, [openAlbumRequest]);

  // Allow external navigation (Search) to open an artist directly.
  useEffect(() => {
    if (!openArtistRequest) return;
    setTab('artists');
    setCurrentArtist(openArtistRequest.artist);
    setDrill({ level: 'artist', artist: openArtistRequest.artist });
  }, [openArtistRequest]);

  // Allow external navigation (Home genres) to open browse with a selected genre filter.
  useEffect(() => {
    if (!openGenreRequest) return;
    const nextSelection = toSingleGenreSelection(openGenreRequest.genre);
    if (!nextSelection.length) return;
    setTab('artists');
    setDrill({ level: 'root' });
    setSelectedGenres(nextSelection);
    setGenreFilterOpen(false);
    setLibraryFilterOpen(false);
  }, [openGenreRequest]);

  // Allow external navigation (sidebar "Browse Music" / library click while drilled in) to reset to root.
  // Seeded with the mount-time value so a stale token left over from an earlier sidebar
  // click doesn't fire on remount and clobber an openAlbumRequest/openArtistRequest that
  // is navigating to a specific album/artist in this same mount.
  const seenResetRequestRef = useRef(resetRequest);
  useEffect(() => {
    if (resetRequest == null) return;
    if (seenResetRequestRef.current === resetRequest) return;
    seenResetRequestRef.current = resetRequest;
    goRoot();
  }, [resetRequest, goRoot]);

  // Build breadcrumb
  const breadcrumb = (() => {
    const crumbs: { label: string; onClick: () => void }[] = [
      { label: 'Browse', onClick: goRoot },
    ];
    if (drill.level === 'artist') {
      crumbs.push({ label: drill.artist.name, onClick: () => {} });
    }
    if (drill.level === 'album') {
      if (drill.artist) {
        crumbs.push({ label: drill.artist.name, onClick: () => goArtist(drill.artist!) });
      }
      crumbs.push({ label: drill.album.title, onClick: () => {} });
    }
    return crumbs;
  })();

  const selectedGenreSet = useMemo(
    () => new Set(selectedGenres.map((value) => value.toLowerCase())),
    [selectedGenres],
  );
  const genreSummary = selectedGenres.length === 0
    ? 'All'
    : `${selectedGenres.length} selected`;
  const selectedLibrarySet = useMemo(
    () => new Set(selectedLibraryIds),
    [selectedLibraryIds],
  );
  const musicLibraries = libraries;
  const forcedLibraryLabel = isLibraryScopeForced && selectedLibraryIds.length === 1
    ? libraries.find((library) => library.id === selectedLibraryIds[0])?.name ?? 'Scoped'
    : null;
  const librarySummary = forcedLibraryLabel
    ? forcedLibraryLabel
    : selectedLibraryIds.length === 0
      ? 'All'
      : `${selectedLibraryIds.length} selected`;

  const genreFilterControl = (
    <div ref={genreFilterRef} style={{ ...L.toggleWrap, ...L.genreWrap }} title="Filter by genre">
      <span style={L.toggleLabel}>Genre</span>
      <button
        style={L.genreTriggerBtn}
        onClick={() => setGenreFilterOpen((open) => !open)}
        aria-label="Genre filter menu"
        aria-haspopup="menu"
        aria-expanded={genreFilterOpen}
      >
        <span style={L.genreTriggerText}>{genreSummary}</span>
        <span style={{ display: 'flex', alignItems: 'center', opacity: 0.7 }}><ChevronDown /></span>
      </button>
      {genreFilterOpen && (
        <div style={L.genrePopover} role="menu" aria-label="Genre filter options">
          <div style={L.genrePopoverHead}>
            <span style={L.genrePopoverTitle}>Select genres</span>
            {selectedGenres.length > 0 && (
              <button style={L.clearFilterBtn} onClick={clearGenreFilter}>
                Clear
              </button>
            )}
          </div>
          <div style={L.genrePopoverList}>
            {genreOptions.length === 0 && (
              <div style={L.genreEmpty}>No genres available.</div>
            )}
            {genreOptions.map((genreItem) => {
              const checked = selectedGenreSet.has(genreItem.genre.toLowerCase());
              return (
                <label key={genreItem.genre} style={L.genreOptionRow}>
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() => toggleGenreFilterSelection(genreItem.genre)}
                  />
                  <span style={L.genreOptionName}>{genreItem.genre}</span>
                  <span style={L.genreOptionCount}>{genreItem.track_count}</span>
                </label>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );

  const libraryFilterControl = (
    <div ref={libraryFilterRef} style={{ ...L.toggleWrap, ...L.genreWrap }} title="Filter by library">
      <span style={L.toggleLabel}>Library</span>
      <button
        style={{ ...L.genreTriggerBtn, ...(isLibraryScopeForced ? L.genreTriggerBtnDisabled : {}) }}
        onClick={() => {
          if (isLibraryScopeForced) return;
          setLibraryFilterOpen((open) => !open);
        }}
        aria-label={isLibraryScopeForced ? 'Library filter locked to sidebar selection' : 'Library filter menu'}
        aria-haspopup="menu"
        aria-expanded={isLibraryScopeForced ? false : libraryFilterOpen}
        disabled={isLibraryScopeForced}
      >
        <span style={L.genreTriggerText}>{librarySummary}</span>
        <span style={{ display: 'flex', alignItems: 'center', opacity: 0.7 }}><ChevronDown /></span>
      </button>
      {isLibraryScopeForced && forcedLibraryLabel && (
        <div style={L.libraryScopeHint}>Sidebar scoped</div>
      )}
      {libraryFilterOpen && !isLibraryScopeForced && (
        <div style={L.genrePopover} role="menu" aria-label="Library filter options">
          <div style={L.genrePopoverHead}>
            <span style={L.genrePopoverTitle}>Select libraries</span>
            {selectedLibraryIds.length > 0 && (
              <button style={L.clearFilterBtn} onClick={clearLibraryFilter}>
                Clear
              </button>
            )}
          </div>
          <div style={L.genrePopoverList}>
            {musicLibraries.length === 0 && (
              <div style={L.genreEmpty}>No libraries available.</div>
            )}
            {musicLibraries.map((library) => (
              <label key={library.id} style={L.genreOptionRow}>
                <input
                  type="checkbox"
                  checked={selectedLibrarySet.has(library.id)}
                  onChange={() => toggleLibraryFilterSelection(library.id)}
                />
                <span style={L.genreOptionName}>{library.name}</span>
              </label>
            ))}
          </div>
        </div>
      )}
    </div>
  );

  const albumSortControl = (
    <div style={L.toggleWrap} title="Sort albums — click active button to flip direction">
      <span style={L.toggleLabel}>Sort</span>
      <div style={L.togglePill}>
        <button
          style={{ ...L.toggleOpt, ...(albumSortField === 'title' ? L.toggleOptActive : {}) }}
          onClick={() => {
            if (albumSortField === 'title') setAlbumSortDir(albumSortDir === 'asc' ? 'desc' : 'asc');
            else setAlbumSortField('title');
          }}
        >
          Name{albumSortField === 'title' ? (albumSortDir === 'asc' ? ' ↑' : ' ↓') : ''}
        </button>
        <button
          style={{ ...L.toggleOpt, ...(albumSortField === 'year' ? L.toggleOptActive : {}) }}
          onClick={() => {
            if (albumSortField === 'year') setAlbumSortDir(albumSortDir === 'asc' ? 'desc' : 'asc');
            else setAlbumSortField('year');
          }}
        >
          Year{albumSortField === 'year' ? (albumSortDir === 'asc' ? ' ↑' : ' ↓') : ''}
        </button>
        <button
          style={{ ...L.toggleOpt, ...(albumSortField === 'rating' ? L.toggleOptActive : {}) }}
          onClick={() => {
            if (albumSortField === 'rating') setAlbumSortDir(albumSortDir === 'asc' ? 'desc' : 'asc');
            else setAlbumSortField('rating');
          }}
        >
          Rating{albumSortField === 'rating' ? (albumSortDir === 'asc' ? ' ↑' : ' ↓') : ''}
        </button>
      </div>
    </div>
  );

  const albumRatingFilterControl = (
    <div style={L.toggleWrap} title="Filter albums by rating">
      <span style={L.toggleLabel}>Rating</span>
      <div style={L.togglePill}>
        {RATING_FILTER_OPTIONS.map(([value, label]) => (
          <button
            key={value}
            style={{ ...L.toggleOpt, ...(albumRatingFilter === value ? L.toggleOptActive : {}) }}
            onClick={() => setAlbumRatingFilter(value)}
          >
            {label}
          </button>
        ))}
      </div>
    </div>
  );

  const artistRatingFilterControl = (
    <div style={L.toggleWrap} title="Filter artists by rating">
      <span style={L.toggleLabel}>Rating</span>
      <div style={L.togglePill}>
        {RATING_FILTER_OPTIONS.map(([value, label]) => (
          <button
            key={value}
            style={{ ...L.toggleOpt, ...(artistRatingFilter === value ? L.toggleOptActive : {}) }}
            onClick={() => setArtistRatingFilter(value)}
          >
            {label}
          </button>
        ))}
      </div>
    </div>
  );

  const artistRefineActive = artistRatingFilter !== 'all'
    || artistSortDir !== 'asc'
    || selectedGenres.length > 0
    || rootViewMode !== 'grid';
  const albumRefineActive = albumRatingFilter !== 'all'
    || albumSortField !== 'title'
    || albumSortDir !== 'asc'
    || selectedGenres.length > 0
    || rootViewMode !== 'grid'
    || groupBy !== 'album_artist';
  const refineActive = tab === 'artists' ? artistRefineActive : albumRefineActive;
  const activeRefinementChips = tab === 'artists'
    ? [
      ...(sonicFingerprintOnly ? [{ key: 'sfp', label: '✦ Sonic Fingerprint', onClear: () => setSonicFingerprintOnly(false) }] : []),
      ...(selectedGenres.length > 0 ? [{ key: 'genre', label: `Genre: ${selectedGenres.length} selected`, onClear: clearGenreFilter }] : []),
      ...(artistRatingFilter !== 'all' ? [{ key: 'rating', label: `Rating: ${getRatingFilterLabel(artistRatingFilter)}`, onClear: () => setArtistRatingFilter('all') }] : []),
      ...(artistSortDir !== 'asc' ? [{ key: 'sort', label: 'Sort: Name ↓', onClear: () => setArtistSortDir('asc') }] : []),
      ...(rootViewMode !== 'grid' ? [{ key: 'view', label: 'View: Table', onClear: () => setPersistedRootViewMode('grid') }] : []),
    ]
    : [
      ...(sonicFingerprintOnly ? [{ key: 'sfp', label: '✦ Sonic Fingerprint', onClear: () => setSonicFingerprintOnly(false) }] : []),
      ...(selectedGenres.length > 0 ? [{ key: 'genre', label: `Genre: ${selectedGenres.length} selected`, onClear: clearGenreFilter }] : []),
      ...(albumRatingFilter !== 'all' ? [{ key: 'rating', label: `Rating: ${getRatingFilterLabel(albumRatingFilter)}`, onClear: () => setAlbumRatingFilter('all') }] : []),
      ...((albumSortField !== 'title' || albumSortDir !== 'asc') ? [{ key: 'sort', label: `Sort: ${getAlbumSortLabel(albumSortField, albumSortDir)}`, onClear: () => { setAlbumSortField('title'); setAlbumSortDir('asc'); } }] : []),
      ...(groupBy !== 'album_artist' ? [{ key: 'groupBy', label: `Group: ${groupBy === 'artist' ? 'Artist' : 'Album Artist'}`, onClear: () => setGroupBy('album_artist') }] : []),
      ...(rootViewMode !== 'grid' ? [{ key: 'view', label: 'View: Table', onClear: () => setPersistedRootViewMode('grid') }] : []),
    ];

  const refineControl = (
    <div ref={refinePanelRef} style={{ ...L.toggleWrap, ...L.refineWrap }} title="Browse refinements">
      <button
        style={{ ...L.compactButton, ...(refineActive || refineOpen ? L.compactButtonActive : {}) }}
        onClick={() => setRefineOpen((open) => !open)}
        aria-label="Browse refine options"
        aria-haspopup="dialog"
        aria-expanded={refineOpen}
      >
        Refine
      </button>
      {refineOpen && (
        <div style={L.refinePopover} role="dialog" aria-label="Browse refine options">
          <div style={L.refinePopoverHead}>
            <div>
              <div style={L.refinePopoverTitle}>Refine {tab === 'artists' ? 'Artists' : 'Albums'}</div>
              <div style={L.refinePopoverMeta}>Advanced browse controls live here instead of staying pinned in the header.</div>
            </div>
            <button style={L.clearFilterBtn} onClick={clearBrowseRefinements}>
              Reset
            </button>
          </div>
          <div style={L.refinePopoverBody}>
            <div style={L.refineSection}>
              <div style={L.refineSectionTitle}>Layout</div>
              <div style={L.refineSectionBody}>
                <div style={{ ...L.toggleWrap, padding: 0 }} title={`Switch ${tab} browse layout`}>
                  <span style={L.toggleLabel}>View</span>
                  <div style={L.togglePill}>
                    <button
                      style={{ ...L.toggleOpt, ...(rootViewMode === 'table' ? L.toggleOptActive : {}) }}
                      onClick={() => setPersistedRootViewMode('table')}
                    >
                      Table
                    </button>
                    <button
                      style={{ ...L.toggleOpt, ...(rootViewMode === 'grid' ? L.toggleOptActive : {}) }}
                      onClick={() => setPersistedRootViewMode('grid')}
                    >
                      Grid
                    </button>
                  </div>
                </div>
                {tab === 'albums' && (
                  <div style={{ ...L.toggleWrap, padding: 0 }} title="Choose how albums are grouped">
                    <span style={L.toggleLabel}>Group by</span>
                    <div style={L.togglePill}>
                      <button
                        style={{ ...L.toggleOpt, ...(groupBy === 'artist' ? L.toggleOptActive : {}) }}
                        onClick={() => setGroupBy('artist')}
                      >
                        Artist
                      </button>
                      <button
                        style={{ ...L.toggleOpt, ...(groupBy === 'album_artist' ? L.toggleOptActive : {}) }}
                        onClick={() => setGroupBy('album_artist')}
                      >
                        Album Artist
                      </button>
                    </div>
                  </div>
                )}
              </div>
            </div>
            <div style={L.refineSection}>
              <div style={L.refineSectionTitle}>Sorting</div>
              <div style={L.refineSectionBody}>
                {tab === 'artists' ? (
                  <div style={{ ...L.toggleWrap, padding: 0 }} title="Sort artists by name - click to flip direction">
                    <span style={L.toggleLabel}>Sort</span>
                    <div style={L.togglePill}>
                      <button
                        style={{ ...L.toggleOpt, ...L.toggleOptActive }}
                        onClick={() => setArtistSortDir(artistSortDir === 'asc' ? 'desc' : 'asc')}
                      >
                        Name{artistSortDir === 'asc' ? ' ↑' : ' ↓'}
                      </button>
                    </div>
                  </div>
                ) : albumSortControl}
              </div>
            </div>
            <div style={L.refineSection}>
              <div style={L.refineSectionTitle}>Filters</div>
              <div style={L.refineSectionBody}>
                {tab === 'artists' ? artistRatingFilterControl : albumRatingFilterControl}
                {genreFilterControl}
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );

  return (
    <div
      data-hybrid-preview-surface={hybridPreview ? 'browse' : undefined}
      style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        minHeight: 0,
        ...(hybridPreview ? hybridBrowseStyles.root : {}),
        ...(hybridPreview ? {
          '--browse-card-background': 'transparent',
          '--browse-card-border-color': 'transparent',
          '--browse-card-radius': '14px',
          '--browse-art-border-color': 'transparent',
          '--browse-art-radius': '14px',
          '--browse-art-shadow': 'var(--shadow-subtle)',
          '--browse-art-hover-outline': HYBRID_ARTWORK_HOVER.outline,
          '--browse-art-hover-filter': HYBRID_ARTWORK_HOVER.filter,
          '--browse-row-border': 'none',
          '--browse-row-radius': '10px',
          '--browse-row-margin': '2px 12px',
        } as React.CSSProperties : {}),
      }}
    >
      {/* Tab bar + optional groupBy toggle — only visible at root */}
      {drill.level === 'root' && (
        <>
          <div style={{ ...L.rootHero, ...(hybridPreview ? hybridBrowseStyles.hero : {}) }}>
            <div style={{ ...L.rootHeroInner, ...(hybridPreview ? hybridBrowseStyles.heroInner : {}) }}>
              <div style={L.rootHeroCopy}>
                <div style={L.rootHeroEyebrow}>Collection</div>
                <div style={{ ...L.rootHeroTitle, ...(hybridPreview ? hybridBrowseStyles.heroTitle : {}) }}>Browse Music</div>
                <div style={{ ...L.rootHeroBody, ...(hybridPreview ? hybridBrowseStyles.heroBody : {}) }}>
                  {tab === 'artists'
                    ? 'Move through your library like a portrait wall instead of a utility list. Filters stay close, but the collection leads.'
                    : 'Hey, those are not true vinyl albums, but they sure make a nice wall!'}
                </div>
              </div>
              <div style={{ ...L.rootHeroStats, ...(hybridPreview ? hybridBrowseStyles.heroStats : {}) }}>
                <div style={L.rootHeroStat}>{tab === 'artists' ? `${sortedArtists.length} artists` : `${sortedAlbums.length} albums`}</div>
                <div style={L.rootHeroStatMuted}>{selectedGenres.length ? `${selectedGenres.length} genres active` : 'Full library view'}</div>
              </div>
            </div>
          </div>
          <div style={{ ...L.rootToolbar, ...(hybridPreview ? hybridBrowseStyles.toolbar : {}) }}>
            <div style={L.rootToolbarLeft}>
              {(['artists', 'albums'] as BrowseTab[]).map(t => (
                <button
                  key={t}
                  style={{ ...L.tab, ...(tab === t ? L.tabActive : {}) }}
                  onClick={() => setTab(t)}
                >
                  {t.charAt(0).toUpperCase() + t.slice(1)}
                </button>
              ))}
            </div>
            <div style={L.rootToolbarRight}>
              <button
                style={{
                  ...L.compactButton,
                  ...(sonicFingerprintOnly ? {
                    ...L.compactButtonActive,
                    color: 'var(--accent)',
                    borderColor: 'color-mix(in srgb, var(--accent) 50%, var(--border))',
                  } : {}),
                }}
                onClick={() => setSonicFingerprintOnly(v => !v)}
                title="Show only artists/albums with Sonic Fingerprint (AI stem analysis)"
                aria-pressed={sonicFingerprintOnly}
              >
                ✦ Sonic Fingerprint
              </button>
              {tab === 'artists' && canEditMetadata && (
                <button
                  style={{
                    ...L.compactButton,
                    ...(artistSelectMode ? {
                      ...L.compactButtonActive,
                      color: 'var(--accent)',
                      borderColor: 'color-mix(in srgb, var(--accent) 50%, var(--border))',
                    } : {}),
                  }}
                  onClick={() => (artistSelectMode ? exitArtistSelectMode() : setArtistSelectMode(true))}
                  title="Select artists to merge duplicate entries"
                  aria-pressed={artistSelectMode}
                >
                  {artistSelectMode ? `Select · ${selectedArtistIds.size} selected` : 'Select'}
                </button>
              )}
              {refineControl}
              {libraryFilterControl}
            </div>
          </div>
          {tab === 'artists' && artistSelectMode && (
            <div style={L.artistSelectBar}>
              <div style={L.artistSelectBarText}>
                <span style={{ fontWeight: 700 }}>{selectedArtistIds.size} artist{selectedArtistIds.size === 1 ? '' : 's'} selected</span>
                <span style={{ color: 'var(--text-muted)' }}> — select 2 or more to merge them into one</span>
              </div>
              <div style={{ display: 'flex', gap: 10 }}>
                <button style={L.artistSelectBarClear} onClick={exitArtistSelectMode}>Clear</button>
                <button
                  style={{ ...L.artistSelectBarMerge, ...(selectedArtistIds.size < 2 ? L.artistSelectBarMergeDisabled : {}) }}
                  disabled={selectedArtistIds.size < 2}
                  onClick={() => setShowMergeModal(true)}
                >
                  Merge {selectedArtistIds.size || ''} Artists
                </button>
              </div>
            </div>
          )}
          {activeRefinementChips.length > 0 && (
            <div style={L.activeChipRow}>
              {activeRefinementChips.map((chip) => (
                <button key={chip.key} style={L.activeChip} onClick={chip.onClear}>
                  <span>{chip.label}</span>
                  <span style={L.activeChipDismiss}>x</span>
                </button>
              ))}
              <button style={L.activeChipClearAll} onClick={clearBrowseRefinements}>
                Clear refine filters
              </button>
            </div>
          )}
        </>
      )}
      {/* Breadcrumb — visible when drilled in */}
      {drill.level !== 'root' && <Breadcrumb items={breadcrumb} />}

      {/* Content */}
      {drill.level === 'root' && tab === 'artists' && (
        rootViewMode === 'table'
          ? <ArtistList artists={sortedArtists} loading={loading} onSelect={goArtist} onPlay={playArtistRadio} alphabeticalJump={artistSortDir === 'asc' && artistRatingFilter === 'all'} sortDir={artistSortDir} initialScrollTop={rootInitialScrollTop} onScrollTopChange={handleRootScrollTopChange} selectMode={artistSelectMode} selectedIds={selectedArtistIds} onToggleSelect={toggleArtistSelected} />
          : <ArtistGrid artists={sortedArtists} loading={loading} onSelect={goArtist} onPlay={playArtistRadio} alphabeticalJump={artistSortDir === 'asc' && artistRatingFilter === 'all'} sortDir={artistSortDir} initialScrollTop={rootInitialScrollTop} initialAnchorId={rootInitialAnchorId} onScrollTopChange={handleRootScrollTopChange} onAnchorChange={handleRootAnchorChange} hybridPreview={hybridPreview} selectMode={artistSelectMode} selectedIds={selectedArtistIds} onToggleSelect={toggleArtistSelected} />
      )}
      {drill.level === 'root' && tab === 'albums' && (
        rootViewMode === 'table'
          ? (
            <AlbumList
              albums={sortedAlbums} loading={loading} showArtist
              groupBy={groupBy}
              alphabeticalJump={albumSortField === 'title' && albumSortDir === 'asc' && albumRatingFilter === 'all'}
              initialScrollTop={rootInitialScrollTop}
              initialAnchorId={rootInitialAnchorId}
              onScrollTopChange={handleRootScrollTopChange}
              onAnchorChange={handleRootAnchorChange}
              onSelect={a => goAlbum(a, null, groupBy)}
              onPlay={playAlbum}
              onQueue={queueAlbum}
            />
          )
          : (
            <AlbumGrid
              albums={sortedAlbums}
              loading={loading}
              onPlay={playAlbum}
              onQueue={queueAlbum}
              onArtistSelect={(artistName) => goArtistByName(artistName)}
              showArtist
              groupBy={groupBy}
              alphabeticalJump={albumSortField === 'title' && albumSortDir === 'asc' && albumRatingFilter === 'all'}
              initialScrollTop={rootInitialScrollTop}
              initialAnchorId={rootInitialAnchorId}
              onScrollTopChange={handleRootScrollTopChange}
              onAnchorChange={handleRootAnchorChange}
              onSelect={a => goAlbum(a, null, groupBy)}
              hybridPreview={hybridPreview}
            />
          )
      )}
      {drill.level === 'artist' && (
        <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
          <div style={L.artistDetailScroll}>
            <ArtistHeader
              artist={currentArtist ?? drill.artist}
              onPlayRadio={() => playArtistRadio(drill.artist)}
              radioLoading={artistRadioLoading}
              onRateArtist={(rating) => handleArtistRatingChange(currentArtist ?? drill.artist, rating)}
              onRefreshed={() => {
                api.artist(drill.artist.id).then(setCurrentArtist).catch(() => {});
              }}
              adaptiveAccentEnabled={adaptiveAccentEnabled}
              canEditMetadata={canEditMetadata}
            />
            <LastFmTopTracks
              artist={drill.artist.name}
              onPlayTopTracks={(tracks) => playTopTracks(drill.artist, tracks)}
              playLoading={topTracksPlayLoading}
            />
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap', marginBottom: 12 }}>
              {albumSortControl}
              {albumRatingFilterControl}
            </div>
            {(artistAlbumSections.album.length > 0 || loading) && (
              <>
                <div style={L.sectionHeading}>Albums</div>
                <AlbumList
                  albums={artistAlbumSections.album} loading={loading}
                  showThumbnail={true}
                  groupBy="artist"
                  fill={false}
                  onSelect={a => goAlbum(a, drill.artist, 'artist')}
                  onPlay={playAlbum}
                  onQueue={queueAlbum}
                />
              </>
            )}
            {artistAlbumSections.single.length > 0 && (
              <>
                <div style={L.sectionHeading}>Singles & EPs</div>
                <AlbumList
                  albums={artistAlbumSections.single}
                  loading={false}
                  showThumbnail={true}
                  groupBy="artist"
                  fill={false}
                  onSelect={a => goAlbum(a, drill.artist, 'artist')}
                  onPlay={playAlbum}
                  onQueue={queueAlbum}
                />
              </>
            )}
            {artistAlbumSections.compilation.length > 0 && (
              <>
                <div style={L.sectionHeading}>Compilations</div>
                <AlbumList
                  albums={artistAlbumSections.compilation}
                  loading={false}
                  showThumbnail={true}
                  groupBy="artist"
                  fill={false}
                  onSelect={a => goAlbum(a, drill.artist, 'artist')}
                  onPlay={playAlbum}
                  onQueue={queueAlbum}
                />
              </>
            )}
            {appearsOnAlbums.length > 0 && (
              <>
                <div style={L.sectionHeading}>Appears On</div>
                <AlbumList
                  albums={sortedAppearsOnAlbums} loading={false}
                  showThumbnail={true}
                  groupBy="album_artist"
                  showArtist
                  fill={false}
                  onSelect={a => goAlbum(a, drill.artist, 'album_artist')}
                  onPlay={playAlbum}
                  onQueue={queueAlbum}
                />
              </>
            )}
            <SimilarArtistsSection artistId={drill.artist.id} onSelect={goArtist} />
          </div>
        </div>
      )}
      {drill.level === 'album' && (
        <TrackList
          tracks={displayedAlbumTracks} loading={loading} album={currentAlbum ?? drill.album} lastfmKey={lastfmKey}
          onPlayTrack={t => playTrack(t, displayedAlbumTracks, { type: 'album', id: drill.album.id })}
          onQueueTrack={t => addToQueue(t)}
          onPlayAll={() => { if (displayedAlbumTracks.length) playTrack(displayedAlbumTracks[0], displayedAlbumTracks, { type: 'album', id: drill.album.id }); }}
          onQueueAll={() => displayedAlbumTracks.forEach(t => addToQueue(t))}
          onPlayVinyl={() => playAlbumInVinylMode(albumTracks, drill.album.id)}
          onRefreshed={(mergedIntoId) => { api.album(mergedIntoId ?? drill.album.id).then(setCurrentAlbum).catch(() => {}); }}
          onRateAlbum={(rating) => handleAlbumRatingChange(currentAlbum ?? drill.album, rating)}
          onRateTrack={handleTrackRatingChange}
          adaptiveAccentEnabled={adaptiveAccentEnabled}
          ratingFilter={trackRatingFilter}
          onRatingFilterChange={setTrackRatingFilter}
          trackSortMode={trackSortMode}
          trackSortDir={trackSortDir}
          onTrackSortModeChange={setTrackSortMode}
          onTrackSortDirChange={setTrackSortDir}
          onArtistClick={() => {
            if (drill.artist) { goArtist(drill.artist); return; }
            const name = drill.album.album_artist || drill.album.artist;
            const found = artists.find(a => a.name === name);
            if (found) { goArtist(found); return; }
            if (name) api.artists().then(all => { const a = all.find(x => x.name === name); if (a) goArtist(a); }).catch(() => {});
          }}
        />
      )}
      {showMergeModal && selectedArtists.length >= 2 && (
        <MergeArtistsModal
          artists={selectedArtists}
          onClose={() => setShowMergeModal(false)}
          onMerged={() => {
            setShowMergeModal(false);
            exitArtistSelectMode();
            api.artists({ genres: selectedGenres, library_ids: activeLibraryIds, sonic_fingerprint_only: sonicFingerprintOnly || undefined, hide_compilation_only: hideCompilationOnlyArtists })
              .then(setArtists)
              .catch(() => {});
          }}
        />
      )}
    </div>
  );
}

// ─── Styles ───────────────────────────────────────────────────────────────────

const L: Record<string, React.CSSProperties> = {
  rootHero: {
    ...phase2.desktopHero,
    flexShrink: 0,
  },
  rootHeroInner: {
    ...phase2.desktopHeroInner,
  },
  rootHeroCopy: {
    minWidth: 260,
    maxWidth: 760,
  },
  rootHeroEyebrow: phase2.eyebrow,
  rootHeroTitle: {
    ...phase2.heroTitle,
  },
  rootHeroBody: {
    ...phase2.heroBody,
  },
  rootHeroStats: {
    ...phase2.tray,
    padding: '14px 16px',
    minWidth: 180,
    display: 'grid',
    gap: 6,
    alignSelf: 'flex-start',
  },
  rootHeroStat: {
    fontSize: 20,
    fontWeight: 800,
    color: 'var(--text)',
    letterSpacing: -0.5,
  },
  rootHeroStatMuted: {
    fontSize: 12,
    color: 'var(--text-muted)',
    fontWeight: 600,
  },
  tabBar: { display: 'flex', borderBottom: '1px solid var(--border)', flexShrink: 0 },
  rootToolbar: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    borderBottom: '1px solid color-mix(in srgb, var(--border) 62%, transparent)',
    flexShrink: 0,
    flexWrap: 'wrap',
    padding: '14px 24px',
    background: 'color-mix(in srgb, var(--surface) 26%, transparent)',
  },
  rootToolbarLeft: {
    display: 'flex',
    alignItems: 'stretch',
  },
  rootToolbarRight: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
    marginLeft: 'auto',
    flexWrap: 'wrap',
  },
  // Artist consolidation multi-select contextual bar (see
  // wip/artist-consolidation-implementation-plan.md).
  artistSelectBar: {
    display: 'flex', alignItems: 'center', justifyContent: 'space-between',
    padding: '12px 24px',
    background: 'color-mix(in srgb, var(--accent) 10%, var(--surface))',
    borderBottom: '1px solid color-mix(in srgb, var(--accent) 28%, var(--border))',
    flexShrink: 0, fontSize: 12.5, color: 'var(--text)',
  },
  artistSelectBarText: { display: 'flex', alignItems: 'center', gap: 8 },
  artistSelectBarClear: {
    padding: '8px 16px', background: 'var(--bg)', border: '1px solid var(--border)',
    borderRadius: 999, color: 'var(--text-muted)', cursor: 'pointer', fontSize: 12,
    fontFamily: 'inherit', fontWeight: 700,
  },
  artistSelectBarMerge: {
    padding: '8px 16px', background: 'var(--accent)', border: 'none',
    borderRadius: 999, color: '#fff', cursor: 'pointer', fontSize: 12,
    fontFamily: 'inherit', fontWeight: 700,
    boxShadow: '0 0 0 1px color-mix(in srgb, var(--accent) 52%, transparent), 0 4px 12px color-mix(in srgb, var(--accent) 30%, transparent)',
  },
  artistSelectBarMergeDisabled: {
    opacity: 0.5, cursor: 'not-allowed', boxShadow: 'none',
  },
  tab: {
    padding: '10px 16px', background: 'rgba(255,255,255,0.03)', border: '1px solid color-mix(in srgb, var(--border) 74%, transparent)',
    color: 'var(--text-muted)', cursor: 'pointer', fontSize: 13,
    fontFamily: 'inherit', borderRadius: 999, fontWeight: 700,
  },
  tabActive: { color: 'var(--text)', background: 'color-mix(in srgb, var(--accent) 15%, transparent)', border: '1px solid color-mix(in srgb, var(--accent) 30%, var(--border))' },
  toggleWrap: {
    display: 'flex', alignItems: 'center', gap: 8,
    padding: '0 16px',
  },
  refineWrap: {
    position: 'relative',
    paddingLeft: 0,
    paddingRight: 0,
  },
  toggleLabel: {
    fontSize: 11, color: 'var(--text-muted)',
    textTransform: 'uppercase' as const, letterSpacing: 0.7, whiteSpace: 'nowrap' as const,
  },
  compactButton: {
    padding: '9px 14px',
    background: 'rgba(255,255,255,0.04)',
    color: 'var(--text)',
    border: '1px solid color-mix(in srgb, var(--border) 76%, transparent)',
    borderRadius: 999,
    cursor: 'pointer',
    fontSize: 12,
    fontFamily: 'inherit',
    whiteSpace: 'nowrap',
    fontWeight: 700,
  },
  compactButtonActive: {
    background: 'color-mix(in srgb, var(--accent) 15%, transparent)',
    color: 'var(--text)',
    border: '1px solid color-mix(in srgb, var(--accent) 30%, var(--border))',
  },
  togglePill: {
    display: 'flex', backgroundColor: 'var(--bg)',
    border: '1px solid var(--border)', borderRadius: 6, overflow: 'hidden',
  },
  toggleOpt: {
    padding: '5px 12px', backgroundColor: 'transparent', border: 'none',
    color: 'var(--text-muted)', cursor: 'pointer', fontSize: 11,
    fontFamily: 'inherit', whiteSpace: 'nowrap' as const, transition: 'all 0.15s',
  },
  toggleOptActive: {
    backgroundColor: 'var(--accent)', color: '#fff',
  },
  sectionHeading: {
    padding: '16px 20px 8px',
    fontSize: 14,
    fontWeight: 600,
    color: 'var(--text-muted)',
    letterSpacing: 0.5,
    textTransform: 'uppercase',
  },
  genreWrap: {
    position: 'relative',
  },
  genreTriggerBtn: {
    minWidth: 124,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 10,
    padding: '5px 10px',
    background: 'var(--bg)',
    color: 'var(--text)',
    border: '1px solid var(--border)',
    borderRadius: 6,
    cursor: 'pointer',
    fontSize: 11,
    fontFamily: 'inherit',
    whiteSpace: 'nowrap',
  },
  genreTriggerBtnDisabled: {
    opacity: 0.72,
    cursor: 'default',
  },
  genreTriggerText: {
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
  libraryScopeHint: {
    marginTop: 6,
    fontSize: 11,
    color: 'var(--text-muted)',
  },
  genrePopover: {
    position: 'absolute',
    top: 'calc(100% + 6px)',
    right: 16,
    zIndex: 20,
    width: 280,
    maxHeight: 280,
    display: 'flex',
    flexDirection: 'column',
    background: 'var(--surface)',
    border: '1px solid var(--border)',
    borderRadius: 8,
    boxShadow: '0 10px 28px rgba(0, 0, 0, 0.35)',
  },
  genrePopoverHead: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    padding: '8px 10px',
    borderBottom: '1px solid var(--border)',
    gap: 8,
  },
  genrePopoverTitle: {
    fontSize: 11,
    color: 'var(--text-muted)',
    letterSpacing: 0.5,
    textTransform: 'uppercase',
    whiteSpace: 'nowrap',
  },
  genrePopoverList: {
    overflowY: 'auto',
    padding: 8,
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
  },
  genreEmpty: {
    color: 'var(--text-muted)',
    fontSize: 11,
    padding: '4px 2px',
  },
  genreOptionRow: {
    display: 'grid',
    gridTemplateColumns: '16px minmax(0, 1fr) auto',
    alignItems: 'center',
    gap: 8,
    padding: '5px 6px',
    borderRadius: 6,
    cursor: 'pointer',
    color: 'var(--text)',
    fontSize: 12,
  },
  genreOptionName: {
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  genreOptionCount: {
    color: 'var(--text-muted)',
    fontSize: 11,
    fontVariantNumeric: 'tabular-nums',
  },
  clearFilterBtn: {
    padding: '5px 10px',
    background: 'transparent',
    color: 'var(--text-muted)',
    border: '1px solid var(--border)',
    borderRadius: 6,
    cursor: 'pointer',
    fontSize: 11,
    fontFamily: 'inherit',
    whiteSpace: 'nowrap',
  },
  refinePopover: {
    position: 'absolute',
    top: 'calc(100% + 8px)',
    right: 0,
    zIndex: 25,
    width: 460,
    maxWidth: 'min(460px, calc(100vw - 32px))',
    background: 'var(--surface)',
    border: '1px solid var(--border)',
    borderRadius: 8,
    display: 'flex',
    flexDirection: 'column',
    overflow: 'hidden',
  },
  refinePopoverHead: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 12,
    padding: '12px 14px',
    borderBottom: '1px solid var(--border)',
  },
  refinePopoverTitle: {
    fontSize: 13,
    fontWeight: 600,
    color: 'var(--text)',
  },
  refinePopoverMeta: {
    marginTop: 4,
    fontSize: 11,
    lineHeight: 1.4,
    color: 'var(--text-muted)',
    maxWidth: 260,
  },
  refinePopoverBody: {
    padding: 12,
    display: 'grid',
    gap: 12,
  },
  refineSection: {
    display: 'grid',
    gap: 8,
    padding: 12,
    border: '1px solid var(--border)',
    borderRadius: 8,
    background: 'var(--bg)',
  },
  refineSectionTitle: {
    fontSize: 11,
    color: 'var(--text-muted)',
    textTransform: 'uppercase',
    letterSpacing: 0.6,
  },
  refineSectionBody: {
    display: 'grid',
    gap: 10,
  },
  activeChipRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    padding: '8px 16px 10px',
    borderBottom: '1px solid var(--border)',
    flexWrap: 'wrap',
  },
  activeChip: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 8,
    padding: '5px 10px',
    background: 'var(--surface)',
    color: 'var(--text)',
    border: '1px solid var(--border)',
    borderRadius: 999,
    cursor: 'pointer',
    fontSize: 11,
    fontFamily: 'inherit',
  },
  activeChipDismiss: {
    color: 'var(--text-muted)',
    fontSize: 11,
    lineHeight: 1,
  },
  activeChipClearAll: {
    padding: 0,
    background: 'transparent',
    color: 'var(--accent)',
    border: 'none',
    cursor: 'pointer',
    fontSize: 11,
    fontFamily: 'inherit',
  },
  alphaShellFill: { position: 'relative', flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' },
  alphaShellStatic: { position: 'relative' },
  alphaScrollable: {
    paddingRight: 42,
    scrollbarGutter: 'stable',
  },
  alphaRail: {
    position: 'absolute',
    top: 8,
    right: 14,
    bottom: 8,
    width: 18,
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    justifyContent: 'space-between',
    zIndex: 6,
    pointerEvents: 'none',
  },
  alphaRailLetter: {
    width: 18,
    height: 14,
    border: 'none',
    borderRadius: 4,
    background: 'transparent',
    color: 'var(--text-muted)',
    fontSize: 9,
    lineHeight: '14px',
    fontWeight: 600,
    padding: 0,
    cursor: 'pointer',
    fontFamily: 'inherit',
    pointerEvents: 'auto',
  },
  alphaRailLetterActive: {
    background: 'color-mix(in srgb, var(--accent) 18%, transparent)',
    color: 'var(--accent)',
  },
  alphaRailLetterDisabled: {
    opacity: 0.28,
    cursor: 'default',
  },
  list: { flex: 1, overflowY: 'auto', paddingBottom: 0 },
  listStack: { flex: '0 0 auto' },
  artistDetailScroll: { flex: 1, minHeight: 0, overflowY: 'auto' },
  similarArtistsSection: {
    padding: '8px 0 24px',
  },
  similarArtistsGrid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fill, minmax(116px, 1fr))',
    gap: 12,
    padding: '0 20px',
  },
  similarArtistCard: {
    minWidth: 0,
    display: 'grid',
    gap: 8,
    padding: 8,
    border: '1px solid transparent',
    borderRadius: 12,
    background: 'transparent',
    color: 'var(--text)',
    cursor: 'pointer',
    fontFamily: 'inherit',
    textAlign: 'left',
    outline: 'none',
    transition: 'background 120ms ease, border-color 120ms ease, transform 120ms ease',
  },
  similarArtistCardActive: {
    borderColor: 'color-mix(in srgb, var(--accent) 36%, var(--border))',
    background: 'color-mix(in srgb, var(--accent) 8%, var(--surface))',
    transform: 'translateY(-2px)',
  },
  similarArtistArt: {
    width: '100%',
    aspectRatio: '1 / 1',
    overflow: 'hidden',
    borderRadius: '50%',
    background: 'var(--bg)',
    boxShadow: 'var(--shadow-subtle)',
  },
  similarArtistName: {
    overflow: 'hidden',
    fontSize: 13,
    fontWeight: 700,
    lineHeight: 1.25,
    textAlign: 'center',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  gridWrap: {
    flex: 1,
    overflowY: 'auto',
    padding: 16,
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fill, minmax(160px, 1fr))',
    gap: 12,
    alignContent: 'start',
  },
  gridTileBtn: {
    background: 'var(--browse-card-background, var(--surface))',
    border: '1px solid var(--browse-card-border-color, var(--border))',
    borderRadius: 'var(--browse-card-radius, 8px)',
    padding: 10,
    cursor: 'pointer',
    color: 'var(--text)',
    textAlign: 'left',
    fontFamily: 'inherit',
    minWidth: 0,
    // See L.row: same off-screen layout/paint skip for grid tiles. Column width
    // still comes from the grid track (minmax(160px, 1fr)), not from this box's
    // intrinsic size, so containment doesn't disturb the auto-fill column count.
    contentVisibility: 'auto',
    containIntrinsicSize: 'auto 200px',
  },
  gridArt: {
    position: 'relative',
    width: '100%',
    aspectRatio: '1 / 1',
    borderRadius: 'var(--browse-art-radius, 6px)',
    overflow: 'hidden',
    background: 'var(--bg)',
    border: '1px solid var(--browse-art-border-color, var(--border))',
    boxShadow: 'var(--browse-art-shadow, none)',
    transition: 'outline-color 120ms ease, filter 120ms ease',
  },
  gridArtHovered: {
    outline: 'var(--browse-art-hover-outline, none)',
    outlineOffset: -2,
    filter: 'var(--browse-art-hover-filter, none)',
  },
  gridArtHoverOverlay: {
    position: 'absolute',
    inset: 0,
    zIndex: 1,
    borderRadius: 'inherit',
    background: HYBRID_ARTWORK_HOVER.wash,
    pointerEvents: 'none',
    transition: 'opacity 120ms ease',
  },
  gridArtImg: {
    width: '100%',
    height: '100%',
    display: 'block',
    objectFit: 'cover',
  },
  gridArtPlaceholder: {
    width: '100%',
    height: '100%',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    color: 'var(--text-muted)',
    opacity: 0.6,
  },
  gridTitle: {
    marginTop: 8,
    fontSize: 12,
    fontWeight: 600,
    color: 'var(--text)',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  gridArtistLink: {
    marginTop: 4,
    padding: 0,
    border: 'none',
    background: 'transparent',
    color: 'var(--text-muted)',
    fontSize: 11,
    lineHeight: 1.35,
    textAlign: 'left' as const,
    cursor: 'pointer',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap' as const,
  },
  gridRatingWrap: {
    marginTop: 6,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'flex-start',
    width: '100%',
  },
  empty: { padding: 40, textAlign: 'center', color: 'var(--text-muted)', fontSize: 13 },
  row: {
    display: 'flex', alignItems: 'center', gap: 12, padding: '10px 16px',
    cursor: 'pointer', borderBottom: 'var(--browse-row-border, 1px solid var(--border))',
    borderRadius: 'var(--browse-row-radius, 0)',
    margin: 'var(--browse-row-margin, 0)',
    transition: 'background 0.1s',
    // Skip layout/paint for off-screen rows so large libraries (10k+ artists/albums)
    // don't pay full-list layout cost on every render; `auto` remembers each row's
    // real rendered size once measured, so this is a placeholder only for rows that
    // have never been on-screen yet.
    contentVisibility: 'auto',
    containIntrinsicSize: 'auto 46px',
  },
  rowIcon: { color: 'var(--text-muted)', flexShrink: 0, display: 'flex', alignItems: 'center' },
  rowThumb: {
    width: 36,
    height: 36,
    borderRadius: 4,
    overflow: 'hidden',
    border: '1px solid var(--border)',
    background: 'var(--surface)',
  },
  gridArtPlayBtn: {
    position: 'absolute',
    top: '50%',
    left: '50%',
    transform: 'translate(-50%, -50%)',
    width: 44,
    height: 44,
    borderRadius: 999,
    border: '1px solid color-mix(in srgb, var(--accent) 42%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 52%, transparent)',
    backdropFilter: 'blur(1.5px)',
    color: 'var(--accent)',
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    cursor: 'pointer',
    transition: 'opacity 120ms ease',
    zIndex: 2,
  },
  gridArtKebabBtn: {
    position: 'absolute',
    top: 8,
    right: 8,
    zIndex: 2,
    background: 'color-mix(in srgb, var(--surface) 72%, transparent)',
    backdropFilter: 'blur(2px)',
  },
  primaryText: { fontSize: 13, fontWeight: 500, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  secondaryText: { fontSize: 11, color: 'var(--text-muted)', marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  meta: { fontSize: 11, color: 'var(--text-muted)', flexShrink: 0, whiteSpace: 'nowrap' },
  chevron: { color: 'var(--text-muted)', flexShrink: 0, display: 'flex', alignItems: 'center', opacity: 0.4 },
  playBtn: {
    display: 'flex', alignItems: 'center', gap: 5,
    background: 'color-mix(in srgb, var(--accent) 15%, transparent)',
    color: 'var(--accent)', border: '1px solid color-mix(in srgb, var(--accent) 30%, transparent)',
    borderRadius: 5, padding: '4px 10px', cursor: 'pointer',
    fontSize: 11, fontFamily: 'inherit', fontWeight: 600, flexShrink: 0,
  },
  albumHeader: {
    display: 'flex', alignItems: 'flex-start',
    gap: 20, padding: '20px 20px', borderBottom: '1px solid var(--border)',
    flexShrink: 0,
    background: 'linear-gradient(135deg, color-mix(in srgb, var(--accent-primary) 13%, var(--surface)) 0%, var(--surface) 70%)',
    transition: 'background 300ms ease',
  },
  albumTitle: { fontSize: 20, fontWeight: 700, color: 'var(--text)', marginBottom: 6, lineHeight: 1.2 },
  albumRatingRow: { display: 'flex', alignItems: 'center', marginBottom: 8 },
  albumMeta: { fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6 },
  coverBox: {
    flexShrink: 0, borderRadius: 6, overflow: 'hidden',
    backgroundColor: 'var(--border)',
    border: '1px solid var(--border)',
  },
  coverPlaceholder: {
    width: '100%', height: '100%',
    display: 'flex', flexDirection: 'column',
    alignItems: 'center', justifyContent: 'center',
    color: 'var(--text-muted)', opacity: 0.4,
  },
  btnPrimary: {
    display: 'flex', alignItems: 'center', gap: 6,
    backgroundColor: 'var(--accent)', color: '#fff', border: 'none',
    borderRadius: 6, padding: '8px 16px', cursor: 'pointer',
    fontSize: 13, fontFamily: 'inherit', fontWeight: 600,
  },
  btnSecondary: {
    display: 'flex', alignItems: 'center', gap: 6,
    backgroundColor: 'transparent', color: 'var(--text-muted)',
    border: '1px solid var(--border)',
    borderRadius: 6, padding: '7px 14px', cursor: 'pointer',
    fontSize: 13, fontFamily: 'inherit',
  },
  trackRow: {
    display: 'flex', alignItems: 'center', gap: 10, padding: '8px 16px',
    borderBottom: '1px solid var(--border)', transition: 'background 0.1s',
    cursor: 'default',
  },
  trackNum: {
    width: 28, textAlign: 'right', fontSize: 12,
    color: 'var(--text-muted)', flexShrink: 0, fontVariantNumeric: 'tabular-nums',
  },
  playRowBtn: {
    background: 'transparent', border: 'none', color: 'var(--text-muted)',
    cursor: 'pointer', padding: '2px 4px', borderRadius: 4,
    display: 'flex', alignItems: 'center', flexShrink: 0, opacity: 0.6,
  },
  trackRatingCell: {
    minWidth: 90,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    flexShrink: 0,
  },
  trackTitle: {
    fontSize: 13, fontWeight: 500, color: 'var(--text)',
    overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
  },
  // ── Artist header ──
  artistHeader: {
    display: 'flex', alignItems: 'flex-start', gap: 20,
    padding: '20px 20px', borderBottom: '1px solid var(--border)', flexShrink: 0,
    background: 'linear-gradient(135deg, color-mix(in srgb, var(--accent-primary) 13%, var(--surface)) 0%, var(--surface) 70%)',
    transition: 'background 300ms ease',
  },
  artistHeaderLeft: {
    display: 'flex', alignItems: 'center', gap: 14, flexShrink: 0,
    minWidth: 240,
  },
  artistHeaderIcon: {
    width: 64, height: 64, borderRadius: '50%',
    backgroundColor: 'color-mix(in srgb, var(--accent) 12%, transparent)',
    border: '1px solid color-mix(in srgb, var(--accent) 25%, transparent)',
    display: 'flex', alignItems: 'center', justifyContent: 'center',
    color: 'var(--accent)', flexShrink: 0,
  },
  artistHeaderPlaceholder: {
    width: '100%',
    height: '100%',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    color: 'var(--accent)',
  },
  artistPhotoBadge: {
    position: 'absolute',
    right: 4,
    bottom: 4,
    fontSize: 8,
    color: '#ddd',
    background: 'rgba(0,0,0,0.55)',
    borderRadius: 3,
    padding: '1px 4px',
    letterSpacing: 0.2,
  },
  // ── Last.fm bio pane ──
  bioWrap: {
    flex: 1, minWidth: 0, borderLeft: '1px solid var(--border)',
    paddingLeft: 20, display: 'flex', flexDirection: 'column', gap: 10,
  },
  bioNoKey: {
    fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6,
    padding: '4px 0',
  },
  bioLoading: {
    display: 'flex', alignItems: 'center', gap: 8,
    fontSize: 12, color: 'var(--text-muted)',
  },
  bioLoadingDot: {
    width: 6, height: 6, borderRadius: '50%',
    backgroundColor: 'var(--accent)', opacity: 0.6,
    animation: 'pulse 1.2s ease-in-out infinite',
  },
  bioEmpty: { fontSize: 12, color: 'var(--text-muted)', opacity: 0.5 },
  bioStats: { display: 'flex', gap: 20 },
  bioStat: { display: 'flex', flexDirection: 'column', gap: 1 },
  bioStatVal: {
    fontSize: 15, fontWeight: 700, color: 'var(--text)',
    fontVariantNumeric: 'tabular-nums',
  },
  bioStatLbl: {
    fontSize: 10, color: 'var(--text-muted)',
    textTransform: 'uppercase', letterSpacing: 0.8,
  },
  bioTags: { display: 'flex', flexWrap: 'wrap', gap: 5 },
  bioTag: {
    fontSize: 10, padding: '2px 8px', borderRadius: 20,
    backgroundColor: 'color-mix(in srgb, var(--accent) 10%, transparent)',
    color: 'var(--accent)',
    border: '1px solid color-mix(in srgb, var(--accent) 20%, transparent)',
    textTransform: 'lowercase', letterSpacing: 0.3,
  },
  bioText: {
    fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.7,
    display: '-webkit-box', WebkitBoxOrient: 'vertical',
    overflow: 'hidden',
  },
  bioExpandBtn: {
    fontSize: 11, color: 'var(--accent)', background: 'transparent',
    border: 'none', cursor: 'pointer', padding: 0, fontFamily: 'inherit',
    textDecoration: 'underline', textUnderlineOffset: 2,
  },
  bioLink: {
    fontSize: 11, color: 'var(--text-muted)',
    textDecoration: 'none', opacity: 0.6,
  },
  topTracksWrap: {
    borderBottom: '1px solid var(--border)',
    padding: '14px 20px',
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
    flexShrink: 0,
  },
  topTracksTitle: {
    fontSize: 12,
    color: 'var(--text)',
    textTransform: 'uppercase',
    letterSpacing: 0.8,
    fontWeight: 700,
  },
  topTracksHead: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 10,
  },
  topTracksHint: {
    fontSize: 12,
    color: 'var(--text-muted)',
  },
  topTracksList: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  topTrackRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 10,
    border: '1px solid var(--border)',
    borderRadius: 6,
    padding: '7px 10px',
  },
  topTrackRank: {
    width: 18,
    textAlign: 'right',
    fontSize: 11,
    color: 'var(--text-muted)',
    fontVariantNumeric: 'tabular-nums',
    flexShrink: 0,
  },
  topTrackName: {
    fontSize: 13,
    color: 'var(--text)',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
  topTrackLink: {
    fontSize: 13,
    color: 'var(--accent)',
    textDecoration: 'none',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    display: 'block',
  },
  topTrackScrobbles: {
    fontSize: 11,
    color: 'var(--text-muted)',
    flexShrink: 0,
    whiteSpace: 'nowrap',
    fontVariantNumeric: 'tabular-nums',
  },
};

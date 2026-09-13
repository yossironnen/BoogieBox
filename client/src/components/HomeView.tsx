/**
 * Defines the Home View React component and related UI helpers.
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../api';
import { useScanActivityRefresh } from '../hooks/useScanActivityRefresh';
import type { Album, LatestAlbum, Artist, ClientEntityId, Genre, HomeGenreSummary, Library, Stats, Track, Playlist, CrossfadeMode, HomeTopRated } from '../types';
import type { EntityId } from '../entityId';
import { HYBRID_ARTWORK_HOVER, hybridHomeStyles } from '../hybridPreview';
import ArtImage from './ArtImage';

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

// ─── Lazy-loaded album cover (reused from previous implementation) ───────────

function HomeAlbumCover({ albumId, title, size = 150 }: { albumId: ClientEntityId; title: string; size?: number }) {
  const [phase, setPhase] = useState<'idle' | 'loading' | 'image' | 'none'>('idle');
  const containerRef = useRef<HTMLDivElement>(null);
  const hasStarted = useRef(false);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting && !hasStarted.current) {
          hasStarted.current = true;
          observer.disconnect();
          setPhase('loading');
        }
      },
      { rootMargin: '200px' },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (phase === 'idle' && hasStarted.current) {
      setPhase('loading');
    }
  }, [phase, albumId]);

  if (phase === 'image' || phase === 'loading') {
    return (
      <ArtImage
        src={api.albumArtUrl(albumId, 300)}
        alt={title}
        imgStyle={{ width: size, height: size, objectFit: 'cover', display: 'block' }}
        onLoadStateChange={(state) => setPhase(state === 'loaded' ? 'image' : state === 'error' ? 'none' : 'loading')}
      />
    );
  }
  return (
    <div ref={containerRef} style={{
      width: size, height: size,
      backgroundColor: 'var(--bg)', color: 'var(--text-muted)',
      display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 13,
    }}>
      {phase === 'idle' ? '' : 'No Cover'}
    </div>
  );
}

// ─── Widget Card wrapper ─────────────────────────────────────────────────────

function WidgetCard({ title, span, className, titleClassName, hybridDesign = false, children }: {
  title: string;
  span?: boolean;
  className?: string;
  titleClassName?: string;
  hybridDesign?: boolean;
  children: React.ReactNode;
}) {
  const storageKey = `boogiebox-pane-collapsed-${title}`;
  const [collapsed, setCollapsed] = React.useState(() =>
    safeLocalStorageGet(storageKey) === 'true'
  );

  const toggle = () => {
    const next = !collapsed;
    setCollapsed(next);
    safeLocalStorageSet(storageKey, String(next));
  };

  return (
    <div
      className={className}
      style={{
      backgroundColor: 'color-mix(in srgb, var(--surface) 92%, transparent)',
      border: '1px solid color-mix(in srgb, var(--border) 70%, transparent)',
      borderRadius: 18,
      padding: '20px 22px',
      gridColumn: span ? '1 / -1' : undefined,
      minWidth: 0,
      boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.03)',
      ...(hybridDesign ? hybridHomeStyles.card : {}),
    }}
    >
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: collapsed ? 0 : 16 }}>
        <div
          className={titleClassName}
          style={{
          fontSize: 20, fontWeight: 700, color: 'var(--text)',
          letterSpacing: -0.4,
          ...(hybridDesign ? hybridHomeStyles.cardTitle : {}),
        }}
        >
          {title}
        </div>
        <button
          onClick={toggle}
          title={collapsed ? 'Expand' : 'Collapse'}
          style={{
            background: 'none', border: 'none', cursor: 'pointer',
            color: 'var(--text-muted)', padding: '0 4px', lineHeight: 1,
            fontSize: 18, opacity: 0.7,
          }}
        >
          {collapsed ? '▸' : '▾'}
        </button>
      </div>
      {!collapsed && children}
    </div>
  );
}

// ─── Stats Widget ────────────────────────────────────────────────────────────

function TrackStatIcon({ size = 18, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M9 18V5l12-2v13" />
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="16" r="3" />
    </svg>
  );
}

function ArtistStatIcon({ size = 18, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M20 21a8 8 0 0 0-16 0" />
      <circle cx="12" cy="7" r="4" />
    </svg>
  );
}

function AlbumStatIcon({ size = 18, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="12" r="2.5" />
    </svg>
  );
}

function StreakIcon({ size = 14, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M12 2c1 3-2 4-2 7a3 3 0 0 0 6 0c1 2-1 4-1 4a6 6 0 1 1-9-8c0 2 1 3 2 3-1-3 1-5 4-6z" />
    </svg>
  );
}

function ListeningTimeIcon({ size = 14, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l4 2" />
    </svg>
  );
}

function TopArtistIcon({ size = 14, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M12 2 14.4 8.6 21 9.3 16 13.6 17.5 20.2 12 16.7 6.5 20.2 8 13.6 3 9.3 9.6 8.6z" />
    </svg>
  );
}

function GenreIcon({ size = 14, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M20.6 12.4 12.6 20.4a2 2 0 0 1-2.8 0l-6.2-6.2a2 2 0 0 1 0-2.8L11.6 3.4a2 2 0 0 1 1.4-.6H19a2 2 0 0 1 2 2v6.2a2 2 0 0 1-.4 1.4z" />
      <circle cx="16" cy="8" r="1.4" fill="currentColor" stroke="none" />
    </svg>
  );
}

function GenreCollage({ albumIds }: { albumIds: ClientEntityId[] }) {
  const ids = albumIds.slice(0, 4);
  const fallbackCount = Math.max(0, 4 - ids.length);
  return (
    <div style={H.autoDjGenreCollage}>
      {ids.map((albumId) => (
        <div key={albumId} style={H.autoDjGenreCollageTile}>
          <ArtImage src={api.albumArtUrl(albumId, 300)} alt="" imgStyle={H.autoDjGenreCollageArt} />
        </div>
      ))}
      {Array.from({ length: fallbackCount }, (_, index) => (
        <div key={`fallback-${index}`} style={{ ...H.autoDjGenreCollageTile, ...H.autoDjGenreCollageFallback }} />
      ))}
    </div>
  );
}

function AutoDjIcon({ size = 20, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M4 4h4l7 16h5" />
      <path d="M4 20h4l3.5-8" />
      <path d="M17 4h3l-2 2 2 2h-3" />
    </svg>
  );
}

function SettingsIcon({ size = 18, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function PlusIcon({ size = 18, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

function ChevronUpIcon({ size = 14, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="m18 15-6-6-6 6" />
    </svg>
  );
}

function SearchIcon({ size = 14, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <circle cx="11" cy="11" r="8" />
      <path d="m21 21-4.3-4.3" />
    </svg>
  );
}

function CrossfadeModeIcon({ size = 16, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M3 12h4l2-9 4 18 2-9h6" />
    </svg>
  );
}

function ZeroGapModeIcon({ size = 16, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <rect x="3" y="10" width="7" height="4" />
      <rect x="14" y="10" width="7" height="4" />
    </svg>
  );
}

function OffModeIcon({ size = 16, style }: { size?: number; style?: React.CSSProperties }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <line x1="4" y1="4" x2="20" y2="20" />
      <line x1="20" y1="4" x2="4" y2="20" />
    </svg>
  );
}

function StatsWidget({ stats }: { stats: Stats | null }) {
  const items = [
    { label: 'Tracks',  value: stats?.total_tracks?.toLocaleString()  ?? '--', Icon: TrackStatIcon },
    { label: 'Artists', value: stats?.total_artists?.toLocaleString() ?? '--', Icon: ArtistStatIcon },
    { label: 'Albums',  value: stats?.total_albums?.toLocaleString()  ?? '--', Icon: AlbumStatIcon },
  ];
  return (
    <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
      {items.map(({ label, value, Icon }) => (
        <div key={label} style={{
          flex: '1 1 100px', textAlign: 'center', padding: '14px 8px',
          backgroundColor: 'var(--bg)', borderRadius: 8, border: '1px solid var(--border)',
        }}>
          <div style={{
            display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 8,
          }}>
            <Icon size={18} style={{ color: 'var(--accent)', opacity: 0.85 }} />
            <span style={{
              fontSize: 30, fontWeight: 700, color: 'var(--accent)',
              fontVariantNumeric: 'tabular-nums', lineHeight: 1.1,
            }}>{value}</span>
          </div>
          <div style={{
            fontSize: 12, color: 'var(--text-muted)', textTransform: 'uppercase',
            letterSpacing: 1, marginTop: 6,
          }}>{label}</div>
        </div>
      ))}
    </div>
  );
}

// ─── Recent Albums Carousel ──────────────────────────────────────────────────

function PlayIcon({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <polygon points="5,3 19,12 5,21" />
    </svg>
  );
}

const RECENT_ALBUMS_LIMIT = 24;

function RecentAlbumsWidget({
  refreshKey,
  onOpenAlbum,
  onPlayTrack,
  hybridDesign,
}: {
  refreshKey: number;
  onOpenAlbum: (album: Album) => void;
  onPlayTrack: (track: Track, allTracks?: Track[]) => void;
  hybridDesign: boolean;
}) {
  const [albums, setAlbums] = useState<LatestAlbum[]>([]);
  const [loading, setLoading] = useState(true);
  const [hoveredAlbumId, setHoveredAlbumId] = useState<ClientEntityId | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    api.latestAlbums(RECENT_ALBUMS_LIMIT)
      .then((rows) => { if (!cancelled) setAlbums(rows); })
      .catch(() => { if (!cancelled) setAlbums([]); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [refreshKey]);

  // Swap newly scanned albums in place (no loading state) so the carousel does not reset.
  useScanActivityRefresh(useCallback(async () => {
    const rows = await api.latestAlbums(RECENT_ALBUMS_LIMIT);
    setAlbums(rows);
  }, []));

  if (loading) return <div style={H.widgetEmpty}>Loading...</div>;
  if (albums.length === 0) return <div style={H.widgetEmpty}>No albums yet</div>;

  return (
    <div style={{
      display: 'flex', gap: 12, overflowX: 'auto', paddingBottom: 8,
    }}>
      {albums.map(album => (
        <button
          key={album.id}
          onClick={() => onOpenAlbum(album)}
          onMouseEnter={() => setHoveredAlbumId(album.id)}
          onMouseLeave={() => setHoveredAlbumId((prev) => (prev === album.id ? null : prev))}
          title={`${album.title} — ${album.album_artist || album.artist || 'Unknown Artist'}`}
          style={{
            flexShrink: 0, width: 150, border: '1px solid',
            borderColor: hybridDesign
              ? 'transparent'
              : hoveredAlbumId === album.id
              ? 'color-mix(in srgb, var(--accent) 34%, var(--border))'
              : 'var(--border)',
            borderRadius: 8,
            backgroundColor: hybridDesign
              ? 'transparent'
              : hoveredAlbumId === album.id
              ? 'color-mix(in srgb, var(--accent) 12%, var(--bg))'
              : 'var(--bg)',
            overflow: 'hidden',
            cursor: 'pointer', padding: 0, textAlign: 'left',
            fontFamily: 'inherit', color: 'inherit',
          }}
        >
          <div
            data-hybrid-recent-album-art={hybridDesign ? album.id : undefined}
            style={{
              ...H.recentAlbumArtWrap,
              ...(hybridDesign ? H.recentAlbumArtWrapHybrid : {}),
              ...(hybridDesign && hoveredAlbumId === album.id
                ? H.recentAlbumArtWrapHybridHovered
                : {}),
            }}
          >
            <HomeAlbumCover albumId={album.id} title={album.title} size={150} />
            {hybridDesign ? (
              <div
                data-hybrid-art-hover-overlay="recent-album"
                aria-hidden="true"
                style={{
                  ...H.recentAlbumArtHoverOverlay,
                  opacity: hoveredAlbumId === album.id ? 1 : 0,
                }}
              />
            ) : null}
            <span
              role="button"
              aria-label={`Play album ${album.title}`}
              title="Play album"
              style={{
                ...H.recentAlbumPlayBtn,
                opacity: hoveredAlbumId === album.id ? 1 : 0,
                pointerEvents: hoveredAlbumId === album.id ? 'auto' : 'none',
              }}
              onClick={async (e) => {
                e.stopPropagation();
                const tracks = await api.albumTracks(album.id);
                if (tracks.length > 0) onPlayTrack(tracks[0], tracks);
              }}
            >
              <PlayIcon size={14} />
            </span>
          </div>
          <div style={{ padding: '8px 10px' }}>
            <div style={{
              fontSize: 14, fontWeight: 600, color: 'var(--text)',
              overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
            }}>{album.title}</div>
            <div style={{
              fontSize: 13, color: 'var(--text-muted)', marginTop: 2,
              overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
            }}>{album.album_artist || album.artist || 'Unknown Artist'}</div>
          </div>
        </button>
      ))}
    </div>
  );
}

/** Top Artist Aria Label is part of this module's public API. */
export function topArtistAriaLabel(artistName: string): string {
  return `Open artist ${artistName}`;
}

/** Top Rated Album Aria Label is part of this module's public API. */
export function topRatedAlbumAriaLabel(album: Album): string {
  return `Open album ${album.title}`;
}

/** Top Rated Track Aria Label is part of this module's public API. */
export function topRatedTrackAriaLabel(track: Track): string {
  const title = track.title?.trim() || track.file_name;
  return `Play ranked track ${title}`;
}

/** Select Top Genres is part of this module's public API. */
export function selectTopGenres<T extends { track_count: number }>(genres: T[], limit = 10): T[] {
  return [...genres]
    .sort((a, b) => b.track_count - a.track_count)
    .slice(0, limit);
}

/** Top Genre Aria Label is part of this module's public API. */
export function topGenreAriaLabel(genreName: string): string {
  return `Open genre ${genreName}`;
}

/** Select Recently Played Tracks is part of this module's public API. */
export function selectRecentlyPlayedTracks(tracks: Track[], limit = 10): Track[] {
  return tracks
    .filter((track) => Boolean(track.last_played_at))
    .slice(0, limit);
}

/** Recently Played Aria Label is part of this module's public API. */
export function recentlyPlayedAriaLabel(track: Track): string {
  const title = track.title?.trim() || track.file_name;
  return `Play ${title}`;
}

/** Select Top Played Tracks is part of this module's public API. */
export function selectTopPlayedTracks(tracks: Track[], limit = 10): Track[] {
  return [...tracks]
    .filter((track) => Number(track.play_count ?? 0) > 0)
    .sort((a, b) => {
      const countDiff = Number(b.play_count ?? 0) - Number(a.play_count ?? 0);
      if (countDiff !== 0) return countDiff;
      const bPlayed = b.last_played_at ? Date.parse(b.last_played_at) : 0;
      const aPlayed = a.last_played_at ? Date.parse(a.last_played_at) : 0;
      if (bPlayed !== aPlayed) return bPlayed - aPlayed;
      return String(b.id).localeCompare(String(a.id));
    })
    .slice(0, limit);
}

/** Select Most Played Artists is part of this module's public API. */
export function selectMostPlayedArtists(artists: Artist[], limit = 10): Artist[] {
  return [...artists]
    .filter((artist) => Number(artist.play_count ?? 0) > 0)
    .sort((a, b) => {
      const countDiff = Number(b.play_count ?? 0) - Number(a.play_count ?? 0);
      if (countDiff !== 0) return countDiff;
      return a.name.localeCompare(b.name);
    })
    .slice(0, limit);
}

/** Top Played Track Aria Label is part of this module's public API. */
export function topPlayedTrackAriaLabel(track: Track): string {
  const title = track.title?.trim() || track.file_name;
  return `Play ${title}`;
}

/** Most Played Artist Aria Label is part of this module's public API. */
export function mostPlayedArtistAriaLabel(artistName: string): string {
  return `Open artist ${artistName}`;
}

function normalizeHomeArtistName(name: string): string {
  return name.trim().replace(/\s+/g, ' ').toLowerCase();
}

function TopRatedWidget({
  refreshKey,
  onOpenArtist,
  onOpenAlbum,
  onPlayTrack,
}: {
  refreshKey: number;
  onOpenArtist: (artist: Artist) => void;
  onOpenAlbum: (album: Album) => void;
  onPlayTrack: (track: Track, allTracks?: Track[]) => void;
}) {
  const [items, setItems] = useState<HomeTopRated>({ artists: [], albums: [], tracks: [] });
  const [loading, setLoading] = useState(true);
  const displayItems = useMemo(() => ({
    artists: items.artists.slice(0, 3),
    albums: items.albums.slice(0, 3),
    tracks: items.tracks.slice(0, 3),
  }), [items]);

  useEffect(() => {
    setLoading(true);
    api.homeTopRated(3)
      .then((next) => setItems(next))
      .catch(() => setItems({ artists: [], albums: [], tracks: [] }))
      .finally(() => setLoading(false));
  }, [refreshKey]);

  if (loading) return <div style={H.widgetEmpty}>Loading...</div>;
  if (!items.artists.length && !items.albums.length && !items.tracks.length) {
    return <div style={H.widgetEmpty}>Rate some artists, albums, or tracks to build your rankings</div>;
  }

  const renderSectionTitle = (title: string) => <div style={H.topRatedSectionTitle}>{title}</div>;
  const renderRating = (rating: number | null | undefined) => (
    <div style={H.topRatedRating}>{rating != null ? `${rating.toFixed(1)}★` : ''}</div>
  );
  const renderThumb = ({
    alt,
    src,
    fallback,
  }: {
    alt: string;
    src: string | null;
    fallback: string;
  }) => (
    <div style={{ ...H.topRatedThumbWrap, borderRadius: 8 }}>
      {src ? (
        <ArtImage
          src={src}
          alt={alt}
          imgStyle={{ ...H.topRatedThumbImage, borderRadius: 8 }}
        />
      ) : (
        <div style={{ ...H.topRatedThumbFallback, borderRadius: 8 }}>
          {fallback}
        </div>
      )}
    </div>
  );

  return (
    <div style={H.topRatedCard}>
      {displayItems.artists.length > 0 && (
        <div style={H.topRatedSection}>
          {renderSectionTitle('Artists')}
          <div style={H.topRatedSectionRows}>
            {displayItems.artists.map((artist, index) => (
              <button
                key={`artist-${artist.id}`}
                onClick={() => onOpenArtist(artist)}
                className="home-list-hover"
                style={H.topRatedRowButton}
                title={`Open ${artist.name}`}
                aria-label={topArtistAriaLabel(artist.name)}
              >
                <div style={H.topRatedRank}>{index + 1}</div>
                {renderThumb({
                  alt: artist.name,
                  src: api.artistPhotoUrl(artist.id, 300),
                  fallback: 'A',
                })}
                <div style={H.topRatedTextWrap}>
                  <div style={H.topRatedPrimary}>{artist.name}</div>
                  <div style={H.topRatedSecondary}>
                    {`${Number(artist.album_count ?? 0).toLocaleString()} albums • ${Number(artist.track_count ?? 0).toLocaleString()} tracks`}
                  </div>
                </div>
                {renderRating(artist.rating)}
              </button>
            ))}
          </div>
        </div>
      )}
      {displayItems.albums.length > 0 && (
        <div style={H.topRatedSection}>
          {renderSectionTitle('Albums')}
          <div style={H.topRatedSectionRows}>
            {displayItems.albums.map((album, index) => (
              <button
                key={`album-${album.id}-${index}`}
                onClick={() => onOpenAlbum(album)}
                className="home-list-hover"
                style={H.topRatedRowButton}
                title={`Open ${album.title}`}
                aria-label={topRatedAlbumAriaLabel(album)}
              >
                <div style={H.topRatedRank}>{index + 1}</div>
                {renderThumb({
                  alt: album.title,
                  src: api.albumArtUrl(album.id, 300),
                  fallback: 'AL',
                })}
                <div style={H.topRatedTextWrap}>
                  <div style={H.topRatedPrimary}>{album.title}</div>
                  <div style={H.topRatedSecondary}>{album.album_artist || album.artist || 'Unknown Artist'}</div>
                </div>
                {renderRating(album.rating)}
              </button>
            ))}
          </div>
        </div>
      )}
      {displayItems.tracks.length > 0 && (
        <div style={H.topRatedSection}>
          {renderSectionTitle('Tracks')}
          <div style={H.topRatedSectionRows}>
            {displayItems.tracks.map((track, index) => {
              const title = track.title || track.file_name;
              const artist = track.artist || 'Unknown Artist';
              return (
                <button
                  key={`track-${track.id}`}
                  onClick={() => onPlayTrack(track, displayItems.tracks)}
                  className="home-list-hover"
                  style={H.topRatedRowButton}
                  title={`Play ${title}`}
                  aria-label={topRatedTrackAriaLabel(track)}
                >
                  <div style={H.topRatedRank}>{index + 1}</div>
                  {renderThumb({
                    alt: track.album || title,
                    src: track.album_id ? api.albumArtUrl(track.album_id, 300) : null,
                    fallback: 'TR',
                  })}
                  <div style={H.topRatedTextWrap}>
                    <div style={H.topRatedPrimary}>{title}</div>
                    <div style={H.topRatedSecondary}>{track.album ? `${artist} • ${track.album}` : artist}</div>
                  </div>
                  {renderRating(track.rating)}
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

// ─── Genre Breakdown Widget ──────────────────────────────────────────────────

function HomeGenresWidget({
  genres,
  onOpenGenre,
  onBrowseMusic,
}: {
  genres: HomeGenreSummary[];
  onOpenGenre: (genre: string) => void;
  onBrowseMusic: () => void;
}) {
  const [genreAlbumId, setGenreAlbumId] = useState<Record<string, ClientEntityId | null>>({});
  const fetchedGenreThumbLabels = useRef<Set<string>>(new Set());
  // Guards the state update below against only a genuine unmount — NOT
  // against this effect merely re-running. HomeView's own top-level mount
  // effect legitimately fetches `homeGenres` twice in quick succession (an
  // initial load, then once more right after its own post-mount system
  // refresh), each producing a brand-new array — a real data reload, not a
  // spurious re-render. A per-effect-run `cancelled` flag (torn down and
  // recreated on every such reload) discarded the first run's in-flight
  // album-art fetch before it could resolve, and nothing ever retried since
  // `fetchedGenreThumbLabels` had already marked those genres as fetched.
  // Once a per-genre fetch starts, it must always be allowed to land.
  const isMountedRef = useRef(true);
  useEffect(() => () => { isMountedRef.current = false; }, []);

  const items = useMemo(() => selectTopGenres(genres, 6), [genres]);

  // Sample a single random album cover per genre for the row thumbnail —
  // fetched once per genre, not re-picked on every render.
  useEffect(() => {
    const pending = items.filter((item) => !fetchedGenreThumbLabels.current.has(item.label));
    if (!pending.length) return;
    for (const item of pending) fetchedGenreThumbLabels.current.add(item.label);
    Promise.all(pending.map(async (item) => {
      try {
        const albums = await api.albums({ genres: [item.label] });
        if (!albums.length) return { label: item.label, id: null as ClientEntityId | null };
        const pick = albums[Math.floor(Math.random() * albums.length)];
        return { label: item.label, id: pick.id };
      } catch {
        return { label: item.label, id: null as ClientEntityId | null };
      }
    })).then((entries) => {
      if (!isMountedRef.current) return;
      setGenreAlbumId((prev) => {
        const next = { ...prev };
        for (const { label, id } of entries) next[label] = id;
        return next;
      });
    });
  }, [items]);

  if (genres.length === 0) return <div style={H.widgetEmpty}>No genre data yet</div>;

  const max = items[0]?.track_count || 1;

  return (
    <div style={H.genreDiscoveryWrap}>
      <div style={H.genreDiscoveryIntro}>
        Jump into the sounds you actually have.
      </div>
      <div style={H.genreDiscoveryList}>
        {items.map((item) => (
          <button
            key={item.canonical_key}
            onClick={() => onOpenGenre(item.label)}
            className="home-list-hover"
            style={H.genreDiscoveryRow}
            title={`Browse ${item.label}`}
            aria-label={topGenreAriaLabel(item.label)}
          >
            <div style={H.genreDiscoveryThumb}>
              {genreAlbumId[item.label]
                ? <ArtImage src={api.albumArtUrl(genreAlbumId[item.label]!, 300)} alt="" imgStyle={H.genreDiscoveryThumbImg} wrapperStyle={H.genreDiscoveryThumbWrap} />
                : <GenreIcon size={16} style={H.genreDiscoveryIcon} />}
            </div>
            <div style={H.genreDiscoveryText}>
              <div style={H.genreDiscoveryName}>{item.label}</div>
              <div style={H.genreDiscoveryMeta}>
                {`${item.artist_count.toLocaleString()} artists • ${item.album_count.toLocaleString()} albums`}
              </div>
            </div>
            <div style={H.genreDiscoveryBarRail}>
              <div
                style={{
                  ...H.genreDiscoveryBarFill,
                  width: `${Math.max(8, (item.track_count / max) * 100)}%`,
                }}
              />
            </div>
            <div style={H.genreDiscoveryCount}>{item.track_count.toLocaleString()}</div>
          </button>
        ))}
      </div>
      <button
        type="button"
        onClick={onBrowseMusic}
        style={H.genreDiscoveryFooterBtn}
        aria-label="Browse all genres in music"
      >
        Browse all genres
      </button>
    </div>
  );
}

function HomeAutoDjModule({
  quickGenres,
  allGenres,
  onStartAutoDj,
}: {
  quickGenres: HomeGenreSummary[];
  allGenres: Genre[];
  onStartAutoDj: (genres: string[]) => Promise<number>;
}) {
  const [autoDjGenres, setAutoDjGenres] = useState<string[]>([]);
  const [autoDjLoading, setAutoDjLoading] = useState(false);
  const [autoDjStatus, setAutoDjStatus] = useState('');
  const [pickerOpen, setPickerOpen] = useState(false);
  const [optionsOpen, setOptionsOpen] = useState(false);
  const [genreSearch, setGenreSearch] = useState('');
  const [autoDjCfMode, setAutoDjCfMode] = useState<CrossfadeMode>('off');
  const [autoDjCfDuration, setAutoDjCfDuration] = useState(2);
  const [autoDjCfHasOverride, setAutoDjCfHasOverride] = useState(false);
  const [autoDjCfSaving, setAutoDjCfSaving] = useState(false);
  const [genreAlbumIds, setGenreAlbumIds] = useState<Record<string, ClientEntityId[]>>({});
  const fetchedGenreLabels = useRef<Set<string>>(new Set());
  // Guards the state update below against only a genuine unmount, not just
  // this effect re-running — see the matching note in HomeGenresWidget.
  // HomeView legitimately re-fetches `homeGenres` (thus a new `quickGenres`
  // reference) a second time right after mount; a per-run `cancelled` flag
  // discarded the first run's in-flight fetches before they resolved.
  const isMountedRef = useRef(true);
  useEffect(() => () => { isMountedRef.current = false; }, []);

  useEffect(() => {
    api.crossfade.config('autodj', '0').then((config) => {
      setAutoDjCfMode(config.mode);
      setAutoDjCfDuration(config.duration);
      setAutoDjCfHasOverride(config.source === 'override');
    }).catch(() => {});
  }, []);

  // Sample up to 4 random album covers per quick genre for the card collage —
  // fetched once per genre, not re-shuffled on every render.
  useEffect(() => {
    const pending = quickGenres.slice(0, 5).filter((genre) => !fetchedGenreLabels.current.has(genre.label));
    if (!pending.length) return;
    for (const genre of pending) fetchedGenreLabels.current.add(genre.label);
    Promise.all(pending.map(async (genre) => {
      try {
        const albums = await api.albums({ genres: [genre.label] });
        const shuffled = [...albums];
        for (let i = shuffled.length - 1; i > 0; i--) {
          const j = Math.floor(Math.random() * (i + 1));
          [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
        }
        return { label: genre.label, ids: shuffled.slice(0, 4).map((album) => album.id) };
      } catch {
        return { label: genre.label, ids: [] as ClientEntityId[] };
      }
    })).then((entries) => {
      if (!isMountedRef.current) return;
      setGenreAlbumIds((prev) => {
        const next = { ...prev };
        for (const { label, ids } of entries) next[label] = ids;
        return next;
      });
    });
  }, [quickGenres]);

  const toggleAutoDjGenre = (genreName: string) => {
    setAutoDjGenres((current) => (
      current.includes(genreName)
        ? current.filter((value) => value !== genreName)
        : [...current, genreName]
    ));
    setAutoDjStatus('');
  };

  const filteredGenres = useMemo(() => {
    const query = genreSearch.trim().toLowerCase();
    if (!query) return allGenres;
    return allGenres.filter((genre) => genre.genre.toLowerCase().includes(query));
  }, [allGenres, genreSearch]);

  const launchAutoDj = async (selectedGenres: string[]) => {
    if (!selectedGenres.length) {
      setAutoDjStatus('Select at least one genre for Auto DJ.');
      return;
    }
    setAutoDjLoading(true);
    setAutoDjStatus('');
    try {
      const queuedCount = await onStartAutoDj(selectedGenres);
      setAutoDjStatus(`Auto DJ started (${queuedCount.toLocaleString()} tracks queued).`);
    } catch (e: any) {
      setAutoDjStatus(e?.message || 'Failed to start Auto DJ.');
    } finally {
      setAutoDjLoading(false);
    }
  };

  const saveAutoDjCrossfadeOverride = async (mode: CrossfadeMode, duration: number) => {
    setAutoDjCfSaving(true);
    try {
      await api.crossfade.upsertOverride({
        entity_type: 'autodj',
        entity_id: '0',
        mode,
        duration,
      });
      setAutoDjCfHasOverride(true);
    } catch {
      // Keep the control responsive even if saving fails.
    } finally {
      setAutoDjCfSaving(false);
    }
  };

  const resetAutoDjCrossfadeOverride = async () => {
    setAutoDjCfSaving(true);
    try {
      await api.crossfade.removeOverride('autodj', '0');
      const config = await api.crossfade.config('autodj', '0');
      setAutoDjCfMode(config.mode);
      setAutoDjCfDuration(config.duration);
      setAutoDjCfHasOverride(config.source === 'override');
    } catch {
      // Keep the current local values on reset failure.
    } finally {
      setAutoDjCfSaving(false);
    }
  };

  const transitionTitle = autoDjCfMode === 'crossfade'
    ? `Transition options (${autoDjCfDuration}s crossfade)`
    : `Transition options (${autoDjCfMode})`;

  return (
    <div style={H.autoDjPanel}>
      <div style={H.autoDjPanelHead}>
        <div style={H.autoDjHeaderLeft}>
          <div style={H.autoDjHeaderIconWrap}>
            <AutoDjIcon size={20} />
          </div>
          <div>
            <div style={H.autoDjLabel}>Auto DJ</div>
            <div style={H.autoDjIntro}>Start from a genre, then let the queue keep moving.</div>
          </div>
        </div>
        <button
          type="button"
          style={{ ...H.autoDjIconBtn, ...(optionsOpen ? H.autoDjIconBtnActive : {}) }}
          onClick={() => setOptionsOpen((value) => !value)}
          aria-expanded={optionsOpen}
          aria-label="Toggle Home Auto DJ transition options"
          title={transitionTitle}
        >
          <SettingsIcon size={18} />
        </button>
      </div>

      {!pickerOpen && (
        <div style={H.autoDjQuickGrid}>
          {quickGenres.slice(0, 5).map((genre) => (
            <button
              key={genre.canonical_key}
              type="button"
              onClick={() => launchAutoDj([genre.label])}
              disabled={autoDjLoading}
              style={H.autoDjGenreCard}
              aria-label={`Start Home Auto DJ with ${genre.label}`}
              title={`Start Auto DJ with ${genre.label}`}
            >
              {(() => {
                const albumIds = genreAlbumIds[genre.label];
                return albumIds?.length
                  ? <GenreCollage albumIds={albumIds} />
                  : <div style={H.autoDjGenreCardIcon}><GenreIcon size={18} /></div>;
              })()}
              <div style={H.autoDjGenreCardName}>{genre.label}</div>
              <div style={H.autoDjGenreCardCount}>{genre.track_count.toLocaleString()} tracks</div>
            </button>
          ))}
          <button
            type="button"
            onClick={() => setPickerOpen(true)}
            style={H.autoDjMoreCard}
            aria-label="Browse all genres for Home Auto DJ"
            title="Browse all genres"
          >
            <div style={H.autoDjMoreCardIcon}><PlusIcon size={18} /></div>
            <div style={H.autoDjGenreCardName}>More genres</div>
          </button>
        </div>
      )}

      {pickerOpen && (
        <>
          <button
            type="button"
            onClick={() => { setPickerOpen(false); setGenreSearch(''); }}
            style={H.autoDjBrowseToggle}
            aria-expanded={pickerOpen}
            aria-label="Hide genre browser"
          >
            <ChevronUpIcon size={14} />
            Hide genre browser
          </button>

          <div style={H.autoDjPickerBox}>
            <div style={H.autoDjPickerHead}>
              <div style={H.autoDjSearchRow}>
                <SearchIcon size={14} style={{ color: 'var(--text-muted)', flexShrink: 0 }} />
                <input
                  type="text"
                  value={genreSearch}
                  onChange={(event) => setGenreSearch(event.target.value)}
                  placeholder={`Search genres (${allGenres.length} total)`}
                  style={H.autoDjSearchInput}
                  aria-label="Search Home Auto DJ genres"
                />
              </div>
              <span style={H.autoDjPickerCount}>
                {autoDjGenres.length
                  ? `${autoDjGenres.length} genre${autoDjGenres.length === 1 ? '' : 's'} selected`
                  : 'Select one or more genres'}
              </span>
              <button
                type="button"
                onClick={() => launchAutoDj(autoDjGenres)}
                disabled={autoDjLoading || autoDjGenres.length === 0}
                style={{
                  ...H.autoDjStartIconBtn,
                  opacity: (autoDjLoading || autoDjGenres.length === 0) ? 0.5 : 1,
                }}
                aria-label="Start Home Auto DJ from picker"
                title="Start Auto DJ"
              >
                <PlayIcon size={16} />
              </button>
            </div>

            <div style={H.autoDjChipScroll}>
              {filteredGenres.map((genre) => {
                const selected = autoDjGenres.includes(genre.genre);
                return (
                  <button
                    key={genre.genre}
                    type="button"
                    onClick={() => toggleAutoDjGenre(genre.genre)}
                    style={{ ...H.autoDjPickChip, ...(selected ? H.autoDjPickChipSelected : {}) }}
                    aria-pressed={selected}
                    aria-label={`${selected ? 'Remove' : 'Add'} ${genre.genre} ${selected ? 'from' : 'to'} the Home Auto DJ selection`}
                  >
                    <span style={{ ...H.autoDjPickChipIcon, ...(selected ? H.autoDjPickChipIconSelected : {}) }}>
                      <GenreIcon size={16} />
                    </span>
                    {genre.genre}
                    <span style={H.autoDjPickChipCount}>{genre.track_count.toLocaleString()}</span>
                  </button>
                );
              })}
              {filteredGenres.length === 0 && (
                <div style={H.widgetEmpty}>No genres match &quot;{genreSearch}&quot;</div>
              )}
            </div>
          </div>
        </>
      )}

      {autoDjStatus && <div style={H.autoDjStatus}>{autoDjStatus}</div>}

      {optionsOpen && (
        <div style={H.autoDjOptionsPanel}>
          <div style={H.autoDjModeRow}>
            {([
              { value: 'crossfade' as const, label: 'Crossfade', Icon: CrossfadeModeIcon },
              { value: 'zerogap' as const, label: 'Zero-gap', Icon: ZeroGapModeIcon },
              { value: 'off' as const, label: 'Off', Icon: OffModeIcon },
            ]).map((option) => (
              <button
                key={option.value}
                type="button"
                onClick={() => {
                  setAutoDjCfMode(option.value);
                  saveAutoDjCrossfadeOverride(option.value, autoDjCfDuration);
                }}
                style={{
                  ...H.autoDjModeBtn,
                  ...(autoDjCfMode === option.value ? H.autoDjModeBtnActive : {}),
                }}
                aria-label={`Set Home Auto DJ transition mode ${option.label}`}
                aria-pressed={autoDjCfMode === option.value}
              >
                <option.Icon size={16} />
                <span>{option.label}</span>
              </button>
            ))}
          </div>

          {autoDjCfMode === 'crossfade' && (
            <div style={H.autoDjDurationRow}>
              <span style={H.autoDjDurationEdge}>1s</span>
              <input
                type="range"
                min={1}
                max={10}
                step={1}
                value={autoDjCfDuration}
                onChange={(event) => {
                  const nextDuration = Number(event.target.value);
                  setAutoDjCfDuration(nextDuration);
                  saveAutoDjCrossfadeOverride(autoDjCfMode, nextDuration);
                }}
                style={H.autoDjDurationSlider}
                aria-label="Home Auto DJ crossfade duration"
              />
              <span style={H.autoDjDurationEdge}>10s</span>
              <span style={H.autoDjDurationValue}>{autoDjCfDuration}s</span>
            </div>
          )}

          <div style={H.autoDjTransitionMeta}>
            {autoDjCfHasOverride ? (
              <button
                type="button"
                onClick={resetAutoDjCrossfadeOverride}
                style={H.autoDjResetBtn}
                aria-label="Reset Home Auto DJ transition override"
              >
                Reset to default
              </button>
            ) : (
              <span style={H.autoDjTransitionHint}>Using global default</span>
            )}
            {autoDjCfSaving && <span style={H.autoDjTransitionHint}>Saving...</span>}
          </div>
        </div>
      )}
    </div>
  );
}

// ─── Quick Playlists Widget ──────────────────────────────────────────────────

type PlaybackActivityTab = 'recently-played' | 'top-played-tracks' | 'most-played-artists';
type BoogieRangeDays = 7 | 30 | 90;

const DAY_MS = 24 * 60 * 60 * 1000;
const BOOGIE_RANGE_OPTIONS: ReadonlyArray<{ value: BoogieRangeDays; label: string }> = [
  { value: 7, label: '7D' },
  { value: 30, label: '30D' },
  { value: 90, label: '90D' },
];

const PLAYBACK_TAB_LABELS: Record<PlaybackActivityTab, string> = {
  'recently-played': 'Recently Played',
  'top-played-tracks': 'Top Played Tracks',
  'most-played-artists': 'Most Played Artists',
};

/** Boogie Snapshot is part of this module's public API. */
export interface BoogieSnapshot {
  dailyCounts: number[];
  dailyLabels: string[];
  dailyDates: number[];
  totalSeconds: number;
  currentStreak: number;
  longestStreak: number;
  topArtist: string;
  topArtistPlays: number;
}

/** Parse Track Timestamp is part of this module's public API. */
export function parseTrackTimestamp(value?: string | null): Date | null {
  if (!value) return null;
  const normalized = value.includes('T') ? value : value.replace(' ', 'T');
  const parsed = new Date(normalized);
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

function startOfDay(value: Date): Date {
  const day = new Date(value);
  day.setHours(0, 0, 0, 0);
  return day;
}

function dayKeyMs(value: Date): number {
  return Date.UTC(value.getFullYear(), value.getMonth(), value.getDate());
}

function dateLabel(value: Date): string {
  return value.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/** Build Boogie Snapshot is part of this module's public API. */
export function buildBoogieSnapshot(
  tracks: Track[],
  rangeDays: BoogieRangeDays,
  now: Date = new Date(),
): BoogieSnapshot {
  const endDay = startOfDay(now);
  const startDay = new Date(endDay);
  startDay.setDate(endDay.getDate() - (rangeDays - 1));
  const startDayKey = dayKeyMs(startDay);
  const endDayKey = dayKeyMs(endDay);
  const counts = Array.from({ length: rangeDays }, () => 0);
  const labels = Array.from({ length: rangeDays }, (_, idx) => {
    const day = new Date(startDay);
    day.setDate(startDay.getDate() + idx);
    return dateLabel(day);
  });
  const dates = Array.from({ length: rangeDays }, (_, idx) => {
    const day = new Date(startDay);
    day.setDate(startDay.getDate() + idx);
    return dayKeyMs(day);
  });
  const artistCounts = new Map<string, number>();
  let totalSeconds = 0;

  for (const track of tracks) {
    const playedAt = parseTrackTimestamp(track.last_played_at ?? null);
    if (!playedAt) continue;
    const playedDayKey = dayKeyMs(startOfDay(playedAt));
    if (playedDayKey < startDayKey || playedDayKey > endDayKey) continue;
    const dayIndex = Math.floor((playedDayKey - startDayKey) / DAY_MS);
    if (dayIndex < 0 || dayIndex >= counts.length) continue;

    counts[dayIndex] += 1;
    if (track.duration && track.duration > 0) {
      totalSeconds += Math.floor(track.duration);
    }

    const artistName = track.artist?.trim() || 'Unknown Artist';
    artistCounts.set(artistName, (artistCounts.get(artistName) || 0) + 1);
  }

  let longestStreak = 0;
  let runningStreak = 0;
  for (const count of counts) {
    if (count > 0) {
      runningStreak += 1;
      if (runningStreak > longestStreak) longestStreak = runningStreak;
    } else {
      runningStreak = 0;
    }
  }

  let currentStreak = 0;
  for (let idx = counts.length - 1; idx >= 0; idx -= 1) {
    if (counts[idx] > 0) {
      currentStreak += 1;
    } else {
      break;
    }
  }

  const topArtistEntry = [...artistCounts.entries()]
    .sort((a, b) => {
      if (b[1] !== a[1]) return b[1] - a[1];
      return a[0].localeCompare(b[0]);
    })[0];

  return {
    dailyCounts: counts,
    dailyLabels: labels,
    dailyDates: dates,
    totalSeconds,
    currentStreak,
    longestStreak,
    topArtist: topArtistEntry?.[0] || 'No artist yet',
    topArtistPlays: topArtistEntry?.[1] || 0,
  };
}

/** Format Minutes is part of this module's public API. */
export function formatMinutes(minutes: number): string {
  if (minutes <= 0) return '0m';
  const hours = Math.floor(minutes / 60);
  const mins = minutes % 60;
  if (!hours) return `${mins}m`;
  return `${hours}h ${mins}m`;
}

function renderCompactMediaThumb({
  alt,
  src,
  shape = 'square',
  fallback,
}: {
  alt: string;
  src: string | null;
  shape?: 'square' | 'circle';
  fallback: string;
}) {
  return (
    <div style={{ ...H.topRatedThumbWrap, borderRadius: shape === 'circle' ? '50%' : 8 }}>
      {src ? (
        <ArtImage
          src={src}
          alt={alt}
          imgStyle={{ ...H.topRatedThumbImage, borderRadius: shape === 'circle' ? '50%' : 8 }}
        />
      ) : (
        <div style={{ ...H.topRatedThumbFallback, borderRadius: shape === 'circle' ? '50%' : 8 }}>
          {fallback}
        </div>
      )}
    </div>
  );
}

function usePrefersReducedMotion(): boolean {
  const [prefersReducedMotion, setPrefersReducedMotion] = useState(false);

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return;
    const media = window.matchMedia('(prefers-reduced-motion: reduce)');
    const onChange = () => setPrefersReducedMotion(media.matches);
    onChange();

    if (typeof media.addEventListener === 'function') {
      media.addEventListener('change', onChange);
      return () => media.removeEventListener('change', onChange);
    }

    media.addListener(onChange);
    return () => media.removeListener(onChange);
  }, []);

  return prefersReducedMotion;
}

function AnimatedMetricNumber({
  value,
  reducedMotion,
  formatter,
  testId,
}: {
  value: number;
  reducedMotion: boolean;
  formatter: (value: number) => string;
  testId?: string;
}) {
  const [displayValue, setDisplayValue] = useState(value);
  const previousValueRef = useRef(value);

  useEffect(() => {
    if (reducedMotion) {
      setDisplayValue(value);
      previousValueRef.current = value;
      return;
    }

    const from = previousValueRef.current;
    const to = value;
    if (from === to) {
      setDisplayValue(to);
      return;
    }

    const durationMs = 400;
    const startTime = performance.now();
    let rafId = 0;

    const animate = (timestamp: number) => {
      const elapsed = timestamp - startTime;
      const progress = Math.min(1, elapsed / durationMs);
      const eased = 1 - Math.pow(1 - progress, 3);
      const nextValue = Math.round(from + ((to - from) * eased));
      setDisplayValue(nextValue);
      if (progress < 1) {
        rafId = window.requestAnimationFrame(animate);
      } else {
        previousValueRef.current = to;
      }
    };

    rafId = window.requestAnimationFrame(animate);
    return () => window.cancelAnimationFrame(rafId);
  }, [reducedMotion, value]);

  return <span data-testid={testId}>{formatter(displayValue)}</span>;
}

function RecentlyPlayedWidget({
  allGenres,
  homeGenres,
  onPlayTrack,
  onOpenArtist,
  onStartAutoDj,
}: {
  allGenres: Genre[];
  homeGenres: HomeGenreSummary[];
  onPlayTrack: (track: Track, allTracks?: Track[]) => void;
  onOpenArtist: (artist: Artist) => void;
  onStartAutoDj: (genres: string[]) => Promise<number>;
}) {
  const [tab, setTab] = useState<PlaybackActivityTab>('recently-played');
  const [recentTracks, setRecentTracks] = useState<Track[]>([]);
  const [heatmapTracks, setHeatmapTracks] = useState<Track[]>([]);
  const [topTracks, setTopTracks] = useState<Track[]>([]);
  const [topArtists, setTopArtists] = useState<Artist[]>([]);
  const [recentLoading, setRecentLoading] = useState(true);
  const [topTracksLoading, setTopTracksLoading] = useState(false);
  const [topArtistsLoading, setTopArtistsLoading] = useState(false);
  const [rangeDays, setRangeDays] = useState<BoogieRangeDays>(30);
  const [boogieTransitionKey, setBoogieTransitionKey] = useState(0);
  const fetchSeqRef = useRef(0);
  const reducedMotion = usePrefersReducedMotion();

  const boogieSnapshot = useMemo(
    () => buildBoogieSnapshot(heatmapTracks, rangeDays),
    [rangeDays, heatmapTracks],
  );
  const totalMinutes = Math.round(boogieSnapshot.totalSeconds / 60);
  const canOpenTopArtist = boogieSnapshot.topArtistPlays > 0 && boogieSnapshot.topArtist !== 'No artist yet';

  const openTopArtist = async () => {
    if (!canOpenTopArtist) return;
    const targetName = boogieSnapshot.topArtist;
    try {
      const artists = await api.artists();
      const match = artists.find((artist) => (
        normalizeHomeArtistName(artist.name) === normalizeHomeArtistName(targetName)
      ));
      if (match) onOpenArtist(match);
    } catch {
      // Ignore lookup failures from this shortcut.
    }
  };

  useEffect(() => {
    const fetchSeq = ++fetchSeqRef.current;
    const isStale = () => fetchSeq !== fetchSeqRef.current;

    if (tab === 'recently-played') {
      setRecentLoading(true);
      api.recentlyPlayed(500).then(all => {
        if (isStale()) return;
        setHeatmapTracks(all);
        setRecentTracks(selectRecentlyPlayedTracks(all, 10));
      }).catch(() => {
        if (isStale()) return;
        setHeatmapTracks([]);
        setRecentTracks([]);
      }).finally(() => {
        if (isStale()) return;
        setRecentLoading(false);
      });
      return;
    }

    if (tab === 'top-played-tracks') {
      setTopTracksLoading(true);
      api.topPlayedTracks(10).then(all => {
        if (isStale()) return;
        setTopTracks(selectTopPlayedTracks(all, 10));
      }).catch(() => {
        if (isStale()) return;
        setTopTracks([]);
      }).finally(() => {
        if (isStale()) return;
        setTopTracksLoading(false);
      });
      return;
    }

    setTopArtistsLoading(true);
    api.mostPlayedArtists(10).then(all => {
      if (isStale()) return;
      setTopArtists(selectMostPlayedArtists(all, 10));
    }).catch(() => {
      if (isStale()) return;
      setTopArtists([]);
    }).finally(() => {
      if (isStale()) return;
      setTopArtistsLoading(false);
    });
  }, [tab]);

  const fmtDur = (seconds: number | null) => {
    if (!seconds || seconds <= 0) return '';
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    return `${m}:${String(s).padStart(2, '0')}`;
  };

  const renderTrackRows = (tracks: Track[], valueRenderer: (track: Track) => string, aria: (track: Track) => string) => (
    <div style={H.topRatedSectionRows}>
      {tracks.slice(0, 3).map(track => {
        const title = track.title || track.file_name;
        const artist = track.artist || 'Unknown Artist';
        const album = track.album || '';
        return (
          <button
            key={track.id}
            onClick={() => onPlayTrack(track)}
            className="home-list-hover"
            style={H.topRatedRowButton}
            title={`Play ${title}`}
            aria-label={aria(track)}
          >
            {renderCompactMediaThumb({
              alt: album || title,
              src: track.album_id ? api.albumArtUrl(track.album_id, 300) : null,
              fallback: 'TR',
            })}
            <div style={H.topRatedTextWrap}>
              <div style={{ ...H.topRatedPrimary, display: 'flex', alignItems: 'center', gap: 4 }}>
                <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{title}</span>
                {track.has_deep_analysis && (
                  <span style={{ fontSize: 11, color: 'var(--accent)', opacity: 0.55, flexShrink: 0 }} title="Sonic Fingerprint available">✦</span>
                )}
              </div>
              <div style={H.topRatedSecondary}>
                {album ? `${artist} • ${album}` : artist}
              </div>
            </div>
            <div style={H.topRatedRating}>
              {valueRenderer(track)}
            </div>
          </button>
        );
      })}
    </div>
  );

  const renderArtistRows = (artists: Artist[]) => (
    <div style={H.topRatedSectionRows}>
      {artists.slice(0, 3).map(artist => {
        const plays = Number(artist.play_count ?? 0);
        const playsLabel = plays.toLocaleString();
        return (
          <button
            key={artist.id}
            onClick={() => onOpenArtist(artist)}
            className="home-list-hover"
            style={H.topRatedRowButton}
            title={`Open ${artist.name}`}
            aria-label={mostPlayedArtistAriaLabel(artist.name)}
          >
            {renderCompactMediaThumb({
              alt: artist.name,
              src: api.artistPhotoUrl(artist.id, 300),
              shape: 'circle',
              fallback: 'A',
            })}
            <div style={H.topRatedTextWrap}>
              <div style={H.topRatedPrimary}>{artist.name}</div>
              <div style={H.topRatedSecondary}>
                {`${Number(artist.album_count ?? 0).toLocaleString()} albums • ${Number(artist.track_count ?? 0).toLocaleString()} tracks`}
              </div>
            </div>
            <div style={{ ...H.topRatedRating, width: 84 }}>
              {playsLabel} plays
            </div>
          </button>
        );
      })}
    </div>
  );

  const isLoading = tab === 'recently-played'
    ? recentLoading
    : tab === 'top-played-tracks'
      ? topTracksLoading
      : topArtistsLoading;

  if (isLoading) return <div style={H.widgetEmpty}>Loading...</div>;

  return (
    <div
      className="boogie-panel"
      data-testid="boogie-visual-section"
      data-transition-key={boogieTransitionKey}
      data-reduced-motion={reducedMotion ? 'true' : 'false'}
      style={{ display: 'flex', flexDirection: 'column', gap: 10 }}
    >
      <div style={H.boogieRangeRow}>
        <div style={H.boogieRangeLabel}>Date Range</div>
        <div role="radiogroup" aria-label="Boogie date range" style={H.boogieRangeGroup}>
          {BOOGIE_RANGE_OPTIONS.map((option) => {
            const isActive = option.value === rangeDays;
            return (
              <button
                key={option.value}
                type="button"
                role="radio"
                aria-checked={isActive}
                aria-label={`Set Boogie date range to last ${option.value} days`}
                className={`boogie-range-btn ${isActive ? 'is-active' : ''}`}
                onClick={() => {
                  if (option.value === rangeDays) return;
                  setRangeDays(option.value);
                  setBoogieTransitionKey((value) => value + 1);
                }}
              >
                {option.label}
              </button>
            );
          })}
        </div>
      </div>

      <div className="boogie-metrics-grid" style={H.boogieMetricsRow} data-testid="boogie-metrics">
        <div style={H.boogieMetricTile}>
          <StreakIcon size={28} style={H.boogieMetricIcon} />
          <div style={H.boogieMetricBody}>
            <div style={H.boogieMetricLabel}>Current streak</div>
            <div style={H.boogieMetricValue}>
              <AnimatedMetricNumber
                value={boogieSnapshot.currentStreak}
                reducedMotion={reducedMotion}
                formatter={(value) => `${value}`}
                testId="boogie-current-streak"
              />
              <span style={H.boogieMetricUnit}>days</span>
            </div>
          </div>
        </div>
        <div style={H.boogieMetricTile}>
          <StreakIcon size={28} style={H.boogieMetricIcon} />
          <div style={H.boogieMetricBody}>
            <div style={H.boogieMetricLabel}>Longest streak</div>
            <div style={H.boogieMetricValue}>
              <AnimatedMetricNumber
                value={boogieSnapshot.longestStreak}
                reducedMotion={reducedMotion}
                formatter={(value) => `${value}`}
                testId="boogie-longest-streak"
              />
              <span style={H.boogieMetricUnit}>days</span>
            </div>
          </div>
        </div>
        <div style={H.boogieMetricTile}>
          <ListeningTimeIcon size={28} style={H.boogieMetricIcon} />
          <div style={H.boogieMetricBody}>
            <div style={H.boogieMetricLabel}>Listening time</div>
            <div style={H.boogieMetricValue}>
              <AnimatedMetricNumber
                value={totalMinutes}
                reducedMotion={reducedMotion}
                formatter={formatMinutes}
                testId="boogie-total-time"
              />
            </div>
          </div>
        </div>
        <div style={H.boogieMetricTile}>
          <TopArtistIcon size={28} style={H.boogieMetricIcon} />
          <div style={H.boogieMetricBody}>
            <div style={H.boogieMetricLabel}>Top artist</div>
            {canOpenTopArtist ? (
              <button
                type="button"
                style={{ ...H.boogieMetricArtistValue, ...H.boogieMetricArtistButton }}
                title={`Open ${boogieSnapshot.topArtist}`}
                aria-label={topArtistAriaLabel(boogieSnapshot.topArtist)}
                onClick={openTopArtist}
              >
                {boogieSnapshot.topArtist}
              </button>
            ) : (
              <div style={H.boogieMetricArtistValue} title={boogieSnapshot.topArtist}>
                {boogieSnapshot.topArtist}
              </div>
            )}
            <div style={H.boogieMetricSubValue}>
              <AnimatedMetricNumber
                value={boogieSnapshot.topArtistPlays}
                reducedMotion={reducedMotion}
                formatter={(value) => `${value} plays`}
                testId="boogie-top-artist-plays"
              />
            </div>
          </div>
        </div>
      </div>

      <div style={H.tabGroup} role="tablist" aria-label="Playback activity views">
        {(Object.keys(PLAYBACK_TAB_LABELS) as PlaybackActivityTab[]).map((tabId) => {
          const isActive = tabId === tab;
          return (
            <button
              key={tabId}
              type="button"
              role="tab"
              aria-selected={isActive}
              onClick={() => setTab(tabId)}
              style={{
                ...H.tabButton,
                ...(isActive ? H.tabButtonActive : {}),
              }}
            >
              {PLAYBACK_TAB_LABELS[tabId]}
            </button>
          );
        })}
      </div>

      {tab === 'recently-played' && (
        recentTracks.length
          ? renderTrackRows(recentTracks, (track) => fmtDur(track.duration), recentlyPlayedAriaLabel)
          : <div style={H.widgetEmpty}>No recently played songs yet</div>
      )}

      {tab === 'top-played-tracks' && (
        topTracks.length
          ? renderTrackRows(topTracks, (track) => String(Number(track.play_count ?? 0)), topPlayedTrackAriaLabel)
          : <div style={H.widgetEmpty}>No played songs yet</div>
      )}

      {tab === 'most-played-artists' && (
        topArtists.length
          ? renderArtistRows(topArtists)
          : <div style={H.widgetEmpty}>No played artists yet</div>
      )}

      <HomeAutoDjModule
        quickGenres={homeGenres}
        allGenres={allGenres}
        onStartAutoDj={onStartAutoDj}
      />
    </div>
  );
}

function homePlaylistFallbackTiles(count: number): number[] {
  return Array.from({ length: Math.max(0, 4 - count) }, (_, index) => index);
}

function QuickPlaylistsWidget({
  onOpenPlaylist,
  onPlayTrack,
}: {
  onOpenPlaylist: (playlistId: EntityId) => void;
  onPlayTrack: (track: Track, allTracks?: Track[]) => void;
}) {
  const [playlists, setPlaylists] = useState<Array<Pick<Playlist, 'id' | 'name' | 'track_count' | 'total_duration' | 'art_album_ids'>>>([]);
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState('');
  const [hoveredPlaylistId, setHoveredPlaylistId] = useState<EntityId | null>(null);

  useEffect(() => {
    api.playlists.list().then(setPlaylists).finally(() => setLoading(false));
  }, []);

  const createPlaylist = async () => {
    const normalizeName = (value: string) => value.trim().replace(/\s+/g, ' ').toLowerCase();
    const rawName = window.prompt('Playlist name', 'New Playlist');
    if (rawName == null) return;
    const name = rawName.trim();
    if (!name) return;
    const normalized = normalizeName(name);
    if (playlists.some((playlist) => normalizeName(playlist.name) === normalized)) {
      setError('A playlist with this name already exists');
      return;
    }
    setCreating(true);
    setError('');
    try {
      const created = await api.playlists.create(name);
      const playlistId = created?.id;
      if (!playlistId) {
        throw new Error('Could not create playlist');
      }
      setPlaylists(prev => {
        if (prev.some(pl => pl.id === playlistId)) return prev;
        return [{ id: playlistId, name: created.name || name, track_count: 0, total_duration: 0, art_album_ids: [] }, ...prev];
      });
      onOpenPlaylist(playlistId);
    } catch (e: any) {
      setError(e?.message || 'Could not create playlist');
    } finally {
      setCreating(false);
    }
  };

  const newPlaylistCard = (
    <button
      type="button"
      onClick={createPlaylist}
      disabled={creating}
      style={{
        flexShrink: 0, width: 150, border: '1px dashed color-mix(in srgb, var(--border) 90%, transparent)',
        borderRadius: 8, background: 'transparent', overflow: 'hidden',
        cursor: creating ? 'default' : 'pointer', padding: 0, textAlign: 'left',
        fontFamily: 'inherit', color: 'var(--text-muted)', opacity: creating ? 0.7 : 1,
      }}
      aria-label="Create playlist from home"
      title="Create playlist"
    >
      <div style={{ ...H.recentAlbumArtWrap, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <PlusIcon size={26} />
      </div>
      <div style={{ padding: '8px 10px' }}>
        <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-muted)' }}>New Playlist</div>
      </div>
    </button>
  );

  if (loading) return <div style={H.widgetEmpty}>Loading...</div>;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {error && <div style={H.errorText}>{error}</div>}
      <div style={{ display: 'flex', gap: 12, overflowX: 'auto', paddingBottom: 8 }}>
        {newPlaylistCard}
        {playlists.map(pl => {
          const artIds = (typeof pl.art_album_ids === 'string'
            ? (pl.art_album_ids as string).split(',').filter(Boolean)
            : (pl.art_album_ids ?? [])
          ).slice(0, 4);
          const metaText = [
            `${pl.track_count} track${pl.track_count !== 1 ? 's' : ''}`,
            pl.total_duration ? formatMinutes(Math.round(pl.total_duration / 60)) : '',
          ].filter(Boolean).join(' · ');
          return (
            <button
              key={pl.id}
              type="button"
              onClick={() => onOpenPlaylist(pl.id)}
              onMouseEnter={() => setHoveredPlaylistId(pl.id)}
              onMouseLeave={() => setHoveredPlaylistId((prev) => (prev === pl.id ? null : prev))}
              aria-label={`Open playlist ${pl.name}`}
              title={pl.name}
              style={{
                flexShrink: 0, width: 150, border: '1px solid',
                borderColor: hoveredPlaylistId === pl.id ? 'color-mix(in srgb, var(--accent) 34%, var(--border))' : 'var(--border)',
                borderRadius: 8,
                backgroundColor: hoveredPlaylistId === pl.id ? 'color-mix(in srgb, var(--accent) 12%, var(--bg))' : 'var(--bg)',
                overflow: 'hidden', cursor: 'pointer', padding: 0, textAlign: 'left',
                fontFamily: 'inherit', color: 'inherit',
              }}
            >
              <div style={H.recentAlbumArtWrap}>
                <div style={H.homePlaylistCollage} aria-label={`${pl.name} artwork`}>
                  {artIds.map((albumId, index) => (
                    <div key={`${pl.id}-art-${albumId}-${index}`} style={H.playlistCardCollageTile}>
                      <ArtImage src={api.albumArtUrl(albumId, 300)} alt="" imgStyle={H.playlistCardCollageArt} />
                    </div>
                  ))}
                  {homePlaylistFallbackTiles(artIds.length).map((tile) => (
                    <div
                      key={`${pl.id}-fallback-${tile}`}
                      style={{ ...H.playlistCardCollageTile, ...H.playlistCardCollageFallback }}
                      aria-hidden
                    />
                  ))}
                </div>
                <span
                  role="button"
                  aria-label={`Play playlist ${pl.name}`}
                  title="Play playlist"
                  style={{
                    ...H.recentAlbumPlayBtn,
                    opacity: hoveredPlaylistId === pl.id ? 1 : 0,
                    pointerEvents: hoveredPlaylistId === pl.id ? 'auto' : 'none',
                  }}
                  onClick={async (e) => {
                    e.stopPropagation();
                    const tracks = await api.playlists.tracks(pl.id);
                    if (tracks.length > 0) onPlayTrack(tracks[0], tracks);
                  }}
                >
                  <PlayIcon size={14} />
                </span>
              </div>
              <div style={{ padding: '8px 10px' }}>
                <div style={{
                  fontSize: 14, fontWeight: 600, color: 'var(--text)',
                  overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                }}>{pl.name}</div>
                <div style={{
                  fontSize: 13, color: 'var(--text-muted)', marginTop: 2,
                  overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                }}>{metaText}</div>
              </div>
            </button>
          );
        })}
      </div>
      {playlists.length === 0 && <div style={H.widgetEmpty}>No playlists yet.</div>}
    </div>
  );
}

// ─── Recently Added Video Widget ─────────────────────────────────────────────

function VideoPosterCard({
  posterPath,
  title,
  meta,
  onClick,
}: {
  posterPath: string | null | undefined;
  title: string;
  meta: string;
  onClick: () => void;
}) {
  const [hovered, setHovered] = useState(false);
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        flexShrink: 0,
        width: 100,
        border: `1px solid ${hovered ? 'color-mix(in srgb, var(--accent) 34%, var(--border))' : 'var(--border)'}`,
        borderRadius: 8,
        backgroundColor: hovered ? 'color-mix(in srgb, var(--accent) 12%, var(--bg))' : 'var(--bg)',
        overflow: 'hidden', cursor: 'pointer', padding: 0, textAlign: 'left',
        fontFamily: 'inherit', color: 'inherit',
        transition: 'background 0.12s, border-color 0.12s',
      }}
    >
      <div style={{ width: 100, height: 150, overflow: 'hidden', backgroundColor: 'var(--surface)', position: 'relative' }}>
        {posterPath ? (
          <img
            src={posterPath}
            alt=""
            style={{ width: '100%', height: '100%', objectFit: 'cover', display: 'block' }}
          />
        ) : (
          <div style={{
            width: '100%', height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center',
            color: 'var(--text-muted)', fontSize: 12,
          }}>
            No Poster
          </div>
        )}
      </div>
      <div style={{ padding: '6px 8px' }}>
        <div style={{
          fontSize: 13, fontWeight: 600, color: 'var(--text)',
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
        }}>{title}</div>
        <div style={{
          fontSize: 12, color: 'var(--text-muted)', marginTop: 2,
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
        }}>{meta}</div>
      </div>
    </button>
  );
}

// ─── HomeView (Dashboard) ────────────────────────────────────────────────────

/** Home View is part of this module's public API. */
export default function HomeView({
  stats,
  libraries = [],
  refreshKey = 0,
  onOpenAlbum,
  onOpenArtist,
  onOpenGenre,
  onBrowseMusic,
  onOpenPlaylist,
  onPlayTrack,
  onStartAutoDj,
  hybridDesign = false,
}: {
  stats: Stats | null;
  libraries?: Library[];
  refreshKey?: number;
  onOpenAlbum: (album: Album) => void;
  onOpenArtist: (artist: Artist) => void;
  onOpenGenre: (genre: string) => void;
  onBrowseMusic: () => void;
  onOpenPlaylist: (playlistId: EntityId) => void;
  onPlayTrack: (track: Track, allTracks?: Track[]) => void;
  onStartAutoDj: (genres: string[]) => Promise<number>;
  hybridDesign?: boolean;
}) {
  const [allGenres, setAllGenres] = useState<Genre[]>([]);
  const [homeGenres, setHomeGenres] = useState<HomeGenreSummary[]>([]);

  useEffect(() => {
    api.genres().then(setAllGenres).catch(() => setAllGenres([]));
    api.homeGenres(6).then(setHomeGenres).catch(() => setHomeGenres([]));
  }, [refreshKey]);

  if (stats && stats.total_tracks === 0) {
    return <div style={H.empty}>No media found yet. Add a library and run a scan.</div>;
  }

  return (
    <div
      data-ui-design={hybridDesign ? 'hybrid' : undefined}
      style={{ ...H.root, ...(hybridDesign ? hybridHomeStyles.root : {}) }}
    >
      <div style={{ ...H.grid, ...(hybridDesign ? hybridHomeStyles.grid : {}) }}>
        <WidgetCard title="Library" span hybridDesign={hybridDesign}>
          <StatsWidget stats={stats} />
        </WidgetCard>

        <WidgetCard title="Recent Albums" span hybridDesign={hybridDesign}>
          <RecentAlbumsWidget
            refreshKey={refreshKey}
            onOpenAlbum={onOpenAlbum}
            onPlayTrack={onPlayTrack}
            hybridDesign={hybridDesign}
          />
        </WidgetCard>

        <WidgetCard title="Let's Boogie!" className="boogie-section" titleClassName="boogie-title" span hybridDesign={hybridDesign}>
          <RecentlyPlayedWidget
            allGenres={allGenres}
            homeGenres={homeGenres}
            onPlayTrack={onPlayTrack}
            onOpenArtist={onOpenArtist}
            onStartAutoDj={onStartAutoDj}
          />
        </WidgetCard>


        <WidgetCard title="Playlists" span hybridDesign={hybridDesign}>
          <QuickPlaylistsWidget onOpenPlaylist={onOpenPlaylist} onPlayTrack={onPlayTrack} />
        </WidgetCard>

        <WidgetCard title="Top Rated" hybridDesign={hybridDesign}>
          <TopRatedWidget
            refreshKey={refreshKey}
            onOpenArtist={onOpenArtist}
            onOpenAlbum={onOpenAlbum}
            onPlayTrack={onPlayTrack}
          />
        </WidgetCard>

        <WidgetCard title="Genres" hybridDesign={hybridDesign}>
          <HomeGenresWidget
            genres={homeGenres}
            onOpenGenre={onOpenGenre}
            onBrowseMusic={onBrowseMusic}
          />
        </WidgetCard>
      </div>
    </div>
  );
}

// ─── Styles ──────────────────────────────────────────────────────────────────

const H: Record<string, React.CSSProperties> = {
  root: { flex: 1, overflowY: 'auto', padding: 20 },
  grid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, 1fr)',
    gap: 18,
  },
  empty: {
    flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center',
    color: 'var(--text-muted)',
  },
  tabGroup: {
    display: 'flex',
    gap: 6,
    flexWrap: 'wrap',
    padding: 6,
    borderRadius: 999,
    backgroundColor: 'color-mix(in srgb, var(--surface) 74%, var(--bg))',
    border: '1px solid color-mix(in srgb, var(--text-muted) 20%, var(--border))',
  },
  tabButton: {
    border: '1px solid transparent',
    backgroundColor: 'transparent',
    color: 'color-mix(in srgb, var(--text-muted) 82%, var(--accent))',
    borderRadius: 999,
    padding: '7px 12px',
    fontSize: 13,
    fontWeight: 700,
    letterSpacing: 0.2,
    cursor: 'pointer',
    fontFamily: 'inherit',
  },
  tabButtonActive: {
    color: 'color-mix(in srgb, var(--accent) 40%, var(--text))',
    border: '1px solid color-mix(in srgb, var(--accent) 30%, var(--border))',
    backgroundColor: 'color-mix(in srgb, var(--surface) 72%, var(--bg))',
    boxShadow: 'inset 0 0 0 1px color-mix(in srgb, var(--accent) 18%, transparent)',
  },
  listRowButton: {
    display: 'flex',
    alignItems: 'center',
    gap: 10,
    backgroundColor: 'color-mix(in srgb, var(--accent) 8%, var(--bg))',
    border: '1px solid color-mix(in srgb, var(--accent) 28%, var(--border))',
    borderRadius: 8,
    cursor: 'pointer',
    textAlign: 'left',
    padding: '6px 8px',
    fontFamily: 'inherit',
    color: 'var(--text)',
    width: '100%',
  },
  topRatedSection: {
    display: 'flex',
    flexDirection: 'column',
    gap: 5,
    paddingTop: 2,
  },
  topRatedCard: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
  },
  topRatedSectionRows: {
    display: 'flex',
    flexDirection: 'column',
    gap: 5,
  },
  topRatedSectionTitle: {
    color: 'var(--text-muted)',
    fontSize: 14,
    letterSpacing: 0.2,
    fontWeight: 700,
    paddingTop: 1,
  },
  topRatedRank: {
    width: 18,
    color: 'var(--text-muted)',
    fontSize: 13,
    fontVariantNumeric: 'tabular-nums',
    flexShrink: 0,
    textAlign: 'center',
  },
  topRatedRowButton: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    backgroundColor: 'color-mix(in srgb, var(--surface) 70%, var(--bg))',
    border: '1px solid color-mix(in srgb, var(--border) 78%, transparent)',
    borderRadius: 12,
    cursor: 'pointer',
    textAlign: 'left',
    padding: '8px 10px',
    fontFamily: 'inherit',
    color: 'var(--text)',
    width: '100%',
    minWidth: 0,
  },
  topRatedThumbWrap: {
    width: 32,
    height: 32,
    overflow: 'hidden',
    flexShrink: 0,
    border: '1px solid color-mix(in srgb, var(--text-muted) 20%, var(--border))',
    backgroundColor: 'var(--bg)',
  },
  topRatedThumbImage: {
    width: '100%',
    height: '100%',
    objectFit: 'cover',
    display: 'block',
  },
  topRatedThumbFallback: {
    width: '100%',
    height: '100%',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    color: 'var(--text-muted)',
    fontSize: 11,
    fontWeight: 700,
    letterSpacing: 0.4,
  },
  topRatedTextWrap: {
    minWidth: 0,
    flex: 1,
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
  },
  topRatedPrimary: {
    fontSize: 15,
    color: 'var(--text)',
    fontWeight: 600,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  topRatedSecondary: {
    fontSize: 13,
    color: 'var(--text-muted)',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  topRatedRating: {
    width: 52,
    fontSize: 13,
    color: 'var(--text-muted)',
    textAlign: 'right',
    fontVariantNumeric: 'tabular-nums',
    flexShrink: 0,
  },
  listRowTitle: {
    width: 140,
    fontSize: 14,
    color: 'var(--accent)',
    fontWeight: 600,
    textDecoration: 'underline',
    textUnderlineOffset: 2,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    flexShrink: 0,
  },
  listRowSubTitle: {
    flex: 1,
    fontSize: 13,
    color: 'var(--text-muted)',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  listRowValue: {
    width: 48,
    fontSize: 13,
    color: 'var(--text-muted)',
    textAlign: 'right',
    fontVariantNumeric: 'tabular-nums',
    flexShrink: 0,
  },
  listRowChevron: {
    color: 'var(--accent)',
    display: 'flex',
    alignItems: 'center',
    flexShrink: 0,
  },
  createPlaylistBtn: {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    alignSelf: 'flex-start',
    backgroundColor: 'color-mix(in srgb, var(--accent) 18%, var(--surface))',
    color: 'var(--text)',
    border: '1px solid color-mix(in srgb, var(--accent) 24%, var(--border))',
    borderRadius: 999,
    padding: '8px 14px',
    cursor: 'pointer',
    fontSize: 14,
    fontWeight: 700,
    letterSpacing: 0.3,
    fontFamily: 'inherit',
  },
  homePlaylistCollage: {
    width: '100%',
    height: '100%',
    display: 'grid',
    gridTemplateColumns: 'repeat(2, 1fr)',
    gridTemplateRows: 'repeat(2, 1fr)',
    gap: 1,
    background: 'color-mix(in srgb, var(--surface) 86%, var(--bg))',
  },
  playlistCardCollage: {
    width: 34,
    height: 34,
    borderRadius: 8,
    overflow: 'hidden',
    display: 'grid',
    gridTemplateColumns: 'repeat(2, 1fr)',
    gridTemplateRows: 'repeat(2, 1fr)',
    gap: 1,
    flexShrink: 0,
    background: 'color-mix(in srgb, var(--surface) 86%, var(--bg))',
    border: '1px solid color-mix(in srgb, var(--border) 78%, transparent)',
    boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.05)',
  },
  playlistCardCollageTile: {
    minWidth: 0,
    minHeight: 0,
    background: 'color-mix(in srgb, var(--surface) 90%, var(--bg))',
  },
  playlistCardCollageArt: {
    width: '100%',
    height: '100%',
    objectFit: 'cover',
    display: 'block',
  },
  playlistCardCollageFallback: {
    background: 'linear-gradient(135deg, color-mix(in srgb, var(--accent) 20%, var(--surface)) 0%, color-mix(in srgb, var(--text-muted) 16%, var(--bg)) 100%)',
  },
  genreDiscoveryWrap: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
  },
  genreDiscoveryIntro: {
    color: 'var(--text-muted)',
    fontSize: 14,
    lineHeight: 1.5,
  },
  genreDiscoveryList: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
  },
  genreDiscoveryRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 10,
    width: '100%',
    border: '1px solid color-mix(in srgb, var(--border) 82%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 72%, var(--bg))',
    borderRadius: 10,
    padding: '10px 12px',
    cursor: 'pointer',
    textAlign: 'left',
    fontFamily: 'inherit',
    color: 'var(--text)',
  },
  genreDiscoveryIcon: {
    color: 'var(--accent)',
    opacity: 0.85,
    flexShrink: 0,
  },
  genreDiscoveryThumb: {
    width: 32,
    height: 32,
    borderRadius: 8,
    overflow: 'hidden',
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    background: 'color-mix(in srgb, var(--accent) 16%, var(--bg))',
  },
  genreDiscoveryThumbImg: {
    width: '100%',
    height: '100%',
    objectFit: 'cover',
    display: 'block',
  },
  genreDiscoveryThumbWrap: {
    display: 'block',
    width: '100%',
    height: '100%',
  },
  genreDiscoveryText: {
    width: 132,
    minWidth: 0,
  },
  genreDiscoveryName: {
    fontSize: 15,
    fontWeight: 700,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  genreDiscoveryMeta: {
    color: 'var(--text-muted)',
    fontSize: 13,
    marginTop: 3,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  genreDiscoveryBarRail: {
    flex: 1,
    height: 8,
    borderRadius: 999,
    background: 'color-mix(in srgb, var(--surface) 70%, var(--bg))',
    overflow: 'hidden',
  },
  genreDiscoveryBarFill: {
    height: '100%',
    borderRadius: 999,
    background: 'linear-gradient(90deg, color-mix(in srgb, var(--accent) 82%, white) 0%, color-mix(in srgb, var(--accent) 58%, var(--surface)) 100%)',
  },
  genreDiscoveryCount: {
    width: 44,
    flexShrink: 0,
    textAlign: 'right',
    fontSize: 13,
    color: 'var(--text-muted)',
    fontVariantNumeric: 'tabular-nums',
  },
  genreDiscoveryFooterBtn: {
    alignSelf: 'flex-start',
    border: '1px solid color-mix(in srgb, var(--border) 82%, transparent)',
    background: 'transparent',
    color: 'var(--accent)',
    borderRadius: 999,
    padding: '6px 12px',
    cursor: 'pointer',
    fontSize: 14,
    fontWeight: 700,
    fontFamily: 'inherit',
  },
  autoDjPanel: {
    display: 'flex',
    flexDirection: 'column',
    gap: 10,
    marginTop: 6,
    paddingTop: 12,
    borderTop: '1px solid color-mix(in srgb, var(--border) 74%, transparent)',
  },
  autoDjPanelHead: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
  },
  autoDjHeaderLeft: {
    display: 'flex',
    alignItems: 'center',
    gap: 12,
    minWidth: 0,
  },
  autoDjHeaderIconWrap: {
    width: 40,
    height: 40,
    borderRadius: 12,
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    background: 'color-mix(in srgb, var(--accent) 20%, var(--bg))',
    color: 'var(--accent)',
  },
  autoDjLabel: {
    color: 'var(--text)',
    fontSize: 16,
    letterSpacing: -0.2,
    fontWeight: 700,
  },
  autoDjIntro: {
    color: 'var(--text-muted)',
    fontSize: 13,
    lineHeight: 1.4,
    marginTop: 2,
  },
  autoDjIconBtn: {
    width: 34,
    height: 34,
    flexShrink: 0,
    borderRadius: 10,
    border: '1px solid color-mix(in srgb, var(--border) 76%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 78%, var(--bg))',
    color: 'var(--text-muted)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    cursor: 'pointer',
  },
  autoDjIconBtnActive: {
    color: 'var(--accent)',
    borderColor: 'color-mix(in srgb, var(--accent) 60%, var(--border))',
    background: 'color-mix(in srgb, var(--accent) 12%, var(--bg))',
  },
  autoDjQuickGrid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fill, minmax(140px, 1fr))',
    gap: 10,
  },
  autoDjGenreCard: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    gap: 6,
    padding: '14px 8px 10px',
    borderRadius: 14,
    border: '1px solid color-mix(in srgb, var(--border) 76%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 78%, var(--bg))',
    color: 'var(--text)',
    cursor: 'pointer',
    textAlign: 'center',
    fontFamily: 'inherit',
  },
  autoDjGenreCardIcon: {
    width: 34,
    height: 34,
    borderRadius: 10,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    background: 'color-mix(in srgb, var(--accent) 16%, var(--bg))',
    color: 'var(--accent)',
  },
  autoDjGenreCollage: {
    width: 48,
    height: 48,
    borderRadius: 12,
    overflow: 'hidden',
    display: 'grid',
    gridTemplateColumns: 'repeat(2, 1fr)',
    gridTemplateRows: 'repeat(2, 1fr)',
    gap: 1,
    background: 'color-mix(in srgb, var(--border) 60%, transparent)',
  },
  autoDjGenreCollageTile: {
    minWidth: 0,
    minHeight: 0,
    background: 'color-mix(in srgb, var(--surface) 90%, var(--bg))',
  },
  autoDjGenreCollageArt: {
    width: '100%',
    height: '100%',
    objectFit: 'cover',
    display: 'block',
  },
  autoDjGenreCollageFallback: {
    background: 'linear-gradient(135deg, color-mix(in srgb, var(--accent) 20%, var(--surface)) 0%, color-mix(in srgb, var(--text-muted) 16%, var(--bg)) 100%)',
  },
  autoDjGenreCardName: {
    fontSize: 13,
    fontWeight: 700,
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    maxWidth: '100%',
  },
  autoDjGenreCardCount: {
    fontSize: 11,
    color: 'var(--text-muted)',
  },
  autoDjMoreCard: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 6,
    padding: '14px 8px 10px',
    borderRadius: 14,
    border: '1px dashed color-mix(in srgb, var(--border) 90%, transparent)',
    background: 'transparent',
    color: 'var(--text-muted)',
    cursor: 'pointer',
    textAlign: 'center',
    fontFamily: 'inherit',
  },
  autoDjMoreCardIcon: {
    width: 34,
    height: 34,
    borderRadius: 10,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    color: 'var(--text-muted)',
  },
  autoDjBrowseToggle: {
    width: '100%',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
    padding: 10,
    borderRadius: 12,
    border: '1px solid color-mix(in srgb, var(--accent) 45%, var(--border))',
    background: 'transparent',
    color: 'var(--accent)',
    fontSize: 13,
    fontWeight: 600,
    fontFamily: 'inherit',
    cursor: 'pointer',
  },
  autoDjPickerBox: {
    borderRadius: 14,
    border: '1px solid color-mix(in srgb, var(--border) 76%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 60%, var(--bg))',
    padding: 12,
  },
  autoDjPickerHead: {
    display: 'flex',
    alignItems: 'center',
    gap: 12,
    marginBottom: 10,
    flexWrap: 'wrap',
  },
  autoDjSearchRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    background: 'color-mix(in srgb, var(--bg) 70%, var(--surface))',
    border: '1px solid color-mix(in srgb, var(--border) 76%, transparent)',
    borderRadius: 10,
    padding: '7px 10px',
    width: 280,
    maxWidth: '100%',
    flexShrink: 0,
  },
  autoDjSearchInput: {
    background: 'none',
    border: 'none',
    outline: 'none',
    color: 'var(--text)',
    fontSize: 13,
    width: '100%',
    fontFamily: 'inherit',
  },
  autoDjPickerCount: {
    fontSize: 12,
    color: 'var(--text-muted)',
    flex: 1,
  },
  autoDjStartIconBtn: {
    width: 40,
    height: 40,
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: 0,
    borderRadius: 12,
    border: 'none',
    background: 'var(--accent)',
    color: '#fff',
    cursor: 'pointer',
    boxShadow: '0 4px 12px color-mix(in srgb, var(--accent) 38%, transparent)',
  },
  autoDjChipScroll: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 8,
    maxHeight: 220,
    overflowY: 'auto',
  },
  autoDjPickChip: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 6,
    padding: '7px 10px 7px 8px',
    borderRadius: 999,
    border: '1px solid color-mix(in srgb, var(--border) 76%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 78%, var(--bg))',
    color: 'var(--text)',
    fontSize: 12.5,
    fontFamily: 'inherit',
    cursor: 'pointer',
  },
  autoDjPickChipSelected: {
    borderColor: 'color-mix(in srgb, var(--accent) 65%, var(--border))',
    background: 'color-mix(in srgb, var(--accent) 18%, var(--bg))',
  },
  autoDjPickChipIcon: {
    display: 'flex',
    color: 'var(--text-muted)',
  },
  autoDjPickChipIconSelected: {
    color: 'var(--accent)',
  },
  autoDjPickChipCount: {
    color: 'var(--text-muted)',
  },
  autoDjStatus: {
    color: 'var(--text-muted)',
    fontSize: 13,
  },
  autoDjOptionsPanel: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
    padding: 10,
    borderRadius: 10,
    background: 'color-mix(in srgb, var(--surface) 72%, var(--bg))',
    border: '1px solid color-mix(in srgb, var(--border) 72%, transparent)',
  },
  autoDjModeRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    maxWidth: 420,
  },
  autoDjModeBtn: {
    flex: 1,
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    gap: 4,
    padding: '10px 6px',
    borderRadius: 12,
    border: '1px solid color-mix(in srgb, var(--border) 76%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 78%, var(--bg))',
    color: 'var(--text-muted)',
    cursor: 'pointer',
    fontSize: 11.5,
    fontWeight: 600,
    fontFamily: 'inherit',
  },
  autoDjModeBtnActive: {
    color: 'var(--accent)',
    borderColor: 'color-mix(in srgb, var(--accent) 60%, var(--border))',
    background: 'color-mix(in srgb, var(--accent) 14%, var(--bg))',
  },
  autoDjDurationRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    maxWidth: 320,
  },
  autoDjDurationEdge: {
    fontSize: 12,
    color: 'var(--text-muted)',
  },
  autoDjDurationSlider: {
    flex: 1,
    accentColor: 'var(--accent)',
  },
  autoDjDurationValue: {
    fontSize: 13,
    color: 'var(--text)',
    minWidth: 22,
    textAlign: 'right',
  },
  autoDjTransitionMeta: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    minHeight: 16,
  },
  autoDjTransitionHint: {
    fontSize: 12,
    color: 'var(--text-muted)',
  },
  autoDjResetBtn: {
    backgroundColor: 'transparent',
    color: 'var(--text-muted)',
    border: '1px solid var(--border)',
    borderRadius: 5,
    padding: '2px 8px',
    cursor: 'pointer',
    fontSize: 12,
    fontFamily: 'inherit',
  },
  errorText: {
    color: '#ef4444',
    fontSize: 13,
  },
  boogieRangeRow: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 10,
    flexWrap: 'wrap',
  },
  boogieRangeLabel: {
    color: 'var(--text-muted)',
    fontSize: 14,
    letterSpacing: 0.2,
    fontWeight: 700,
  },
  boogieRangeGroup: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
  },
  boogieMetricsRow: {
    display: 'grid',
    gap: 10,
  },
  boogieMetricTile: {
    minWidth: 0,
    display: 'flex',
    alignItems: 'center',
    gap: 12,
    border: '1px solid color-mix(in srgb, var(--border) 76%, transparent)',
    backgroundColor: 'color-mix(in srgb, var(--surface) 78%, var(--bg))',
    borderRadius: 14,
    padding: '10px 12px',
  },
  boogieMetricBody: {
    minWidth: 0,
    flex: '1 1 auto',
  },
  boogieMetricIcon: {
    color: 'var(--accent)',
    opacity: 0.85,
    flexShrink: 0,
  },
  boogieMetricLabel: {
    fontSize: 13,
    fontWeight: 700,
    letterSpacing: 0.2,
    color: 'var(--text-muted)',
  },
  boogieMetricValue: {
    marginTop: 4,
    display: 'flex',
    alignItems: 'baseline',
    gap: 4,
    fontSize: 22,
    color: 'var(--text)',
    fontWeight: 700,
    fontVariantNumeric: 'tabular-nums',
    minHeight: 24,
  },
  boogieMetricUnit: {
    color: 'var(--text-muted)',
    fontSize: 13,
    letterSpacing: 0.2,
  },
  boogieMetricArtistValue: {
    marginTop: 4,
    color: 'var(--text)',
    fontSize: 15,
    fontWeight: 700,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    minHeight: 18,
  },
  boogieMetricArtistButton: {
    display: 'block',
    width: '100%',
    padding: 0,
    border: 'none',
    background: 'transparent',
    textAlign: 'left',
    cursor: 'pointer',
    fontFamily: 'inherit',
    textDecoration: 'underline',
    textUnderlineOffset: 2,
  },
  boogieMetricSubValue: {
    marginTop: 2,
    color: 'var(--text-muted)',
    fontSize: 13,
    minHeight: 14,
  },
  widgetEmpty: {
    color: 'var(--text-muted)', fontSize: 15, padding: '10px 0',
  },
  recentAlbumArtWrap: {
    position: 'relative',
    width: 150,
    height: 150,
  },
  recentAlbumArtWrapHybrid: {
    overflow: 'hidden',
    borderRadius: 14,
    border: '1px solid transparent',
    boxShadow: 'var(--shadow-subtle)',
    transition: 'outline-color 120ms ease, filter 120ms ease',
  },
  recentAlbumArtWrapHybridHovered: {
    outline: HYBRID_ARTWORK_HOVER.outline,
    outlineOffset: -2,
    filter: HYBRID_ARTWORK_HOVER.filter,
  },
  recentAlbumArtHoverOverlay: {
    position: 'absolute',
    inset: 0,
    zIndex: 1,
    borderRadius: 'inherit',
    background: HYBRID_ARTWORK_HOVER.wash,
    pointerEvents: 'none',
    transition: 'opacity 120ms ease',
  },
  recentAlbumPlayBtn: {
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
};

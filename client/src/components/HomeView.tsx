/**
 * Defines the Home View React component and related UI helpers.
 */

import React, { useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../api';
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
      display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 11,
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
          fontSize: 18, fontWeight: 700, color: 'var(--text)',
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
            fontSize: 16, opacity: 0.7,
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

function StatsWidget({ stats }: { stats: Stats | null }) {
  const items = [
    { label: 'Tracks',  value: stats?.total_tracks?.toLocaleString()  ?? '--' },
    { label: 'Artists', value: stats?.total_artists?.toLocaleString() ?? '--' },
    { label: 'Albums',  value: stats?.total_albums?.toLocaleString()  ?? '--' },
  ];
  return (
    <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
      {items.map(({ label, value }) => (
        <div key={label} style={{
          flex: '1 1 100px', textAlign: 'center', padding: '14px 8px',
          backgroundColor: 'var(--bg)', borderRadius: 8, border: '1px solid var(--border)',
        }}>
          <div style={{
            fontSize: 28, fontWeight: 700, color: 'var(--accent)',
            fontVariantNumeric: 'tabular-nums', lineHeight: 1.1,
          }}>{value}</div>
          <div style={{
            fontSize: 10, color: 'var(--text-muted)', textTransform: 'uppercase',
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
    setLoading(true);
    api.latestAlbums(24)
      .then(setAlbums)
      .catch(() => setAlbums([]))
      .finally(() => setLoading(false));
  }, [refreshKey]);

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
              fontSize: 12, fontWeight: 600, color: 'var(--text)',
              overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
            }}>{album.title}</div>
            <div style={{
              fontSize: 11, color: 'var(--text-muted)', marginTop: 2,
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
    shape = 'square',
    fallback,
  }: {
    alt: string;
    src: string | null;
    shape?: 'square' | 'circle';
    fallback: string;
  }) => (
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
                  shape: 'circle',
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
  if (genres.length === 0) return <div style={H.widgetEmpty}>No genre data yet</div>;

  const items = selectTopGenres(genres, 6);
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
  const [autoDjCfMode, setAutoDjCfMode] = useState<CrossfadeMode>('off');
  const [autoDjCfDuration, setAutoDjCfDuration] = useState(2);
  const [autoDjCfHasOverride, setAutoDjCfHasOverride] = useState(false);
  const [autoDjCfSaving, setAutoDjCfSaving] = useState(false);

  useEffect(() => {
    api.crossfade.config('autodj', '0').then((config) => {
      setAutoDjCfMode(config.mode);
      setAutoDjCfDuration(config.duration);
      setAutoDjCfHasOverride(config.source === 'override');
    }).catch(() => {});
  }, []);

  const onAutoDjGenresChange = (event: React.ChangeEvent<HTMLSelectElement>) => {
    const selected = Array.from(event.currentTarget.options)
      .filter((option) => option.selected)
      .map((option) => option.value);
    setAutoDjGenres(selected);
    setAutoDjStatus('');
  };

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

  return (
    <div style={H.autoDjPanel}>
      <div style={H.autoDjPanelHead}>
        <div style={H.autoDjWrap}>
          <div style={H.autoDjLabel}>Auto DJ</div>
          <div style={H.autoDjIntro}>Start from a genre, then let the queue keep moving.</div>
        </div>
        <button
          type="button"
          style={H.autoDjSecondaryBtn}
          onClick={() => setPickerOpen((value) => !value)}
          aria-expanded={pickerOpen}
          aria-label="Toggle more Home Auto DJ genres"
        >
          {pickerOpen ? 'Hide genres' : 'More genres'}
        </button>
      </div>

      <div style={H.autoDjChipRow}>
        {quickGenres.slice(0, 5).map((genre) => (
          <button
            key={genre.canonical_key}
            type="button"
            onClick={() => launchAutoDj([genre.label])}
            disabled={autoDjLoading}
            style={H.autoDjChip}
            aria-label={`Start Home Auto DJ with ${genre.label}`}
          >
            <span>{genre.label}</span>
            <span style={H.autoDjChipCount}>{genre.track_count.toLocaleString()}</span>
          </button>
        ))}
      </div>

      {pickerOpen && (
        <div style={H.autoDjPicker}>
          <select
            multiple
            size={4}
            aria-label="Home Auto DJ genre picker"
            title="Select one or more genres for Auto DJ"
            style={H.autoDjGenreSelect}
            value={autoDjGenres}
            onChange={onAutoDjGenresChange}
          >
            {allGenres.map((genre) => (
              <option key={genre.genre} value={genre.genre}>{genre.genre} ({genre.track_count})</option>
            ))}
          </select>
          <button
            style={{ ...H.createPlaylistBtn, ...H.autoDjStartBtn, opacity: autoDjLoading ? 0.7 : 1 }}
            onClick={() => launchAutoDj(autoDjGenres)}
            disabled={autoDjLoading || allGenres.length === 0}
            aria-label="Start Home Auto DJ from picker"
            title="Build a random queue from selected genres"
          >
            {autoDjLoading ? 'Starting...' : 'Start selected genres'}
          </button>
        </div>
      )}

      {autoDjStatus && <div style={H.autoDjStatus}>{autoDjStatus}</div>}

      <div style={H.autoDjTransitionWrap}>
        <button
          type="button"
          onClick={() => setOptionsOpen((value) => !value)}
          style={H.autoDjOptionsToggle}
          aria-expanded={optionsOpen}
          aria-label="Toggle Home Auto DJ options"
        >
          <span style={H.autoDjTransitionLabel}>Options</span>
          <span style={H.autoDjOptionsMeta}>
            {autoDjCfMode === 'crossfade' ? `${autoDjCfDuration}s crossfade` : autoDjCfMode}
          </span>
        </button>

        {optionsOpen && (
          <div style={H.autoDjOptionsPanel}>
            <div style={H.autoDjModePill}>
              {([
                { value: 'off' as const, label: 'Off' },
                { value: 'zerogap' as const, label: 'Zero-gap' },
                { value: 'crossfade' as const, label: 'Crossfade' },
              ]).map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => {
                    setAutoDjCfMode(option.value);
                    saveAutoDjCrossfadeOverride(option.value, autoDjCfDuration);
                  }}
                  style={{
                    ...H.autoDjModeOption,
                    ...(autoDjCfMode === option.value ? H.autoDjModeOptionActive : {}),
                  }}
                  aria-label={`Set Home Auto DJ transition mode ${option.label}`}
                >
                  {option.label}
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
                  <span style={{ fontSize: 9, color: 'var(--accent)', opacity: 0.55, flexShrink: 0 }} title="Sonic Fingerprint available">✦</span>
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
        <div style={H.boogieMetricTile}>
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
        <div style={H.boogieMetricTile}>
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
        <div style={H.boogieMetricTile}>
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

function QuickPlaylistsWidget({
  onOpenPlaylist,
}: {
  onOpenPlaylist: (playlistId: EntityId) => void;
}) {
  const [playlists, setPlaylists] = useState<Array<Pick<Playlist, 'id' | 'name' | 'track_count' | 'art_album_ids'>>>([]);
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState('');

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
        return [{ id: playlistId, name: created.name || name, track_count: 0, art_album_ids: [] }, ...prev];
      });
      onOpenPlaylist(playlistId);
    } catch (e: any) {
      setError(e?.message || 'Could not create playlist');
    } finally {
      setCreating(false);
    }
  };

  const createBtn = (
    <button
      style={{ ...H.createPlaylistBtn, opacity: creating ? 0.7 : 1 }}
      onClick={createPlaylist}
      disabled={creating}
      aria-label="Create playlist from home"
      title="Create playlist"
    >
      + New Playlist
    </button>
  );

  if (loading) return <div style={H.widgetEmpty}>Loading...</div>;
  if (playlists.length === 0) {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        {createBtn}
        <div style={H.widgetEmpty}>No playlists yet.</div>
        {error && <div style={H.errorText}>{error}</div>}
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {createBtn}
      {error && <div style={H.errorText}>{error}</div>}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      {playlists.map(pl => (
        <button
          key={pl.id}
          type="button"
          onClick={() => onOpenPlaylist(pl.id)}
          style={{
            display: 'flex', alignItems: 'center', gap: 12,
            padding: '8px 12px', borderRadius: 6,
            width: '100%',
            border: 'none',
            background: 'transparent',
            textAlign: 'left',
            cursor: 'pointer',
            fontFamily: 'inherit',
          }}
          aria-label={`Open playlist ${pl.name}`}
          title={pl.name}
        >
          <div style={H.playlistCardCollage} aria-label={`${pl.name} artwork`}>
            {(typeof pl.art_album_ids === 'string'
              ? (pl.art_album_ids as string).split(',').filter(Boolean)
              : (pl.art_album_ids ?? [])
            ).slice(0, 4).map((albumId, index) => (
              <div key={`${pl.id}-art-${albumId}-${index}`} style={H.playlistCardCollageTile}>
                <ArtImage src={api.albumArtUrl(albumId, 300)} alt="" imgStyle={H.playlistCardCollageArt} />
              </div>
            ))}
            {Array.from({ length: Math.max(0, 4 - Math.min(4, typeof pl.art_album_ids === 'string' ? (pl.art_album_ids as string).split(',').filter(Boolean).length : (pl.art_album_ids?.length ?? 0))) }, (_, index) => (
              <div
                key={`${pl.id}-fallback-${index}`}
                style={{ ...H.playlistCardCollageTile, ...H.playlistCardCollageFallback }}
                aria-hidden
              />
            ))}
          </div>
          <div style={{ flex: 1, fontSize: 13, color: 'var(--text)', fontWeight: 500 }}>{pl.name}</div>
          <div style={{ fontSize: 11, color: 'var(--text-muted)', fontVariantNumeric: 'tabular-nums' }}>
            {pl.track_count} tracks
          </div>
        </button>
      ))}
      </div>
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
            color: 'var(--text-muted)', fontSize: 10,
          }}>
            No Poster
          </div>
        )}
      </div>
      <div style={{ padding: '6px 8px' }}>
        <div style={{
          fontSize: 11, fontWeight: 600, color: 'var(--text)',
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
        }}>{title}</div>
        <div style={{
          fontSize: 10, color: 'var(--text-muted)', marginTop: 2,
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
          <QuickPlaylistsWidget onOpenPlaylist={onOpenPlaylist} />
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
    fontSize: 11,
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
    fontSize: 12,
    letterSpacing: 0.2,
    fontWeight: 700,
    paddingTop: 1,
  },
  topRatedRank: {
    width: 18,
    color: 'var(--text-muted)',
    fontSize: 11,
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
    fontSize: 9,
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
    fontSize: 13,
    color: 'var(--text)',
    fontWeight: 600,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  topRatedSecondary: {
    fontSize: 11,
    color: 'var(--text-muted)',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  topRatedRating: {
    width: 52,
    fontSize: 11,
    color: 'var(--text-muted)',
    textAlign: 'right',
    fontVariantNumeric: 'tabular-nums',
    flexShrink: 0,
  },
  listRowTitle: {
    width: 140,
    fontSize: 12,
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
    fontSize: 11,
    color: 'var(--text-muted)',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  listRowValue: {
    width: 48,
    fontSize: 11,
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
    fontSize: 12,
    fontWeight: 700,
    letterSpacing: 0.3,
    fontFamily: 'inherit',
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
  autoDjWrap: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  genreDiscoveryWrap: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
  },
  genreDiscoveryIntro: {
    color: 'var(--text-muted)',
    fontSize: 12,
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
  genreDiscoveryText: {
    width: 132,
    minWidth: 0,
  },
  genreDiscoveryName: {
    fontSize: 13,
    fontWeight: 700,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  genreDiscoveryMeta: {
    color: 'var(--text-muted)',
    fontSize: 11,
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
    fontSize: 11,
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
    fontSize: 12,
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
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 12,
  },
  autoDjLabel: {
    color: 'var(--text-muted)',
    fontSize: 12,
    letterSpacing: 0.2,
    fontWeight: 700,
  },
  autoDjIntro: {
    color: 'var(--text-muted)',
    fontSize: 12,
    lineHeight: 1.45,
    marginTop: 3,
  },
  autoDjSecondaryBtn: {
    border: '1px solid color-mix(in srgb, var(--border) 82%, transparent)',
    background: 'transparent',
    color: 'var(--text)',
    borderRadius: 999,
    padding: '6px 10px',
    cursor: 'pointer',
    fontSize: 11,
    fontWeight: 700,
    fontFamily: 'inherit',
    whiteSpace: 'nowrap',
  },
  autoDjChipRow: {
    display: 'flex',
    gap: 8,
    flexWrap: 'wrap',
  },
  autoDjChip: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 8,
    border: '1px solid color-mix(in srgb, var(--accent) 32%, var(--border))',
    background: 'color-mix(in srgb, var(--accent) 10%, var(--surface))',
    color: 'var(--text)',
    borderRadius: 999,
    padding: '7px 12px',
    cursor: 'pointer',
    fontSize: 12,
    fontWeight: 700,
    fontFamily: 'inherit',
  },
  autoDjChipCount: {
    color: 'var(--text-muted)',
    fontSize: 11,
    fontWeight: 600,
  },
  autoDjPicker: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
    padding: 10,
    borderRadius: 10,
    background: 'color-mix(in srgb, var(--surface) 72%, var(--bg))',
    border: '1px solid color-mix(in srgb, var(--border) 72%, transparent)',
  },
  autoDjGenreSelect: {
    backgroundColor: 'var(--bg)',
    border: '1px solid var(--border)',
    color: 'var(--text)',
    borderRadius: 6,
    padding: '6px 8px',
    fontSize: 12,
    fontFamily: 'inherit',
    minHeight: 94,
  },
  autoDjStartBtn: {
    alignSelf: 'flex-start',
  },
  autoDjStatus: {
    color: 'var(--text-muted)',
    fontSize: 11,
  },
  autoDjTransitionWrap: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    paddingTop: 2,
  },
  autoDjOptionsToggle: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 10,
    width: '100%',
    border: '1px solid color-mix(in srgb, var(--border) 82%, transparent)',
    background: 'transparent',
    color: 'var(--text)',
    borderRadius: 10,
    padding: '8px 10px',
    cursor: 'pointer',
    fontFamily: 'inherit',
  },
  autoDjTransitionLabel: {
    color: 'var(--text-muted)',
    fontSize: 12,
    letterSpacing: 0.2,
    fontWeight: 700,
  },
  autoDjOptionsMeta: {
    color: 'var(--text-muted)',
    fontSize: 11,
    textTransform: 'capitalize',
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
  autoDjModePill: {
    display: 'flex',
    border: '1px solid var(--border)',
    borderRadius: 7,
    overflow: 'hidden',
    width: 'fit-content',
  },
  autoDjModeOption: {
    border: 'none',
    borderRight: '1px solid var(--border)',
    backgroundColor: 'var(--bg)',
    color: 'var(--text)',
    cursor: 'pointer',
    padding: '5px 10px',
    fontSize: 11,
    fontWeight: 600,
    fontFamily: 'inherit',
  },
  autoDjModeOptionActive: {
    backgroundColor: 'var(--accent)',
    color: '#fff',
  },
  autoDjDurationRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    maxWidth: 320,
  },
  autoDjDurationEdge: {
    fontSize: 10,
    color: 'var(--text-muted)',
  },
  autoDjDurationSlider: {
    flex: 1,
    accentColor: 'var(--accent)',
  },
  autoDjDurationValue: {
    fontSize: 11,
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
    fontSize: 10,
    color: 'var(--text-muted)',
  },
  autoDjResetBtn: {
    backgroundColor: 'transparent',
    color: 'var(--text-muted)',
    border: '1px solid var(--border)',
    borderRadius: 5,
    padding: '2px 8px',
    cursor: 'pointer',
    fontSize: 10,
    fontFamily: 'inherit',
  },
  errorText: {
    color: '#ef4444',
    fontSize: 11,
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
    fontSize: 12,
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
    border: '1px solid color-mix(in srgb, var(--border) 76%, transparent)',
    backgroundColor: 'color-mix(in srgb, var(--surface) 78%, var(--bg))',
    borderRadius: 14,
    padding: '10px 12px',
  },
  boogieMetricLabel: {
    fontSize: 11,
    fontWeight: 700,
    letterSpacing: 0.2,
    color: 'var(--text-muted)',
  },
  boogieMetricValue: {
    marginTop: 4,
    display: 'flex',
    alignItems: 'baseline',
    gap: 4,
    fontSize: 20,
    color: 'var(--text)',
    fontWeight: 700,
    fontVariantNumeric: 'tabular-nums',
    minHeight: 24,
  },
  boogieMetricUnit: {
    color: 'var(--text-muted)',
    fontSize: 11,
    letterSpacing: 0.2,
  },
  boogieMetricArtistValue: {
    marginTop: 4,
    color: 'var(--text)',
    fontSize: 13,
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
    fontSize: 11,
    minHeight: 14,
  },
  widgetEmpty: {
    color: 'var(--text-muted)', fontSize: 13, padding: '10px 0',
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

/**
 * Defines mobile Mobile Playlists View behavior for the BoogieBox React client.
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../../api';
import type { ClientEntityId, Playlist, PlaylistTrack, Track } from '../../types';
import type { EntityId } from '../../entityId';
import type { MobilePlaylistSelection } from '../mobileShell';
import ArtImage from '../../components/ArtImage';
import { hybridMobileContentStyles } from '../../hybridPreview';
import { phase2 } from '../../uiPhase2';
import {
  MobilePlaylistEditorSheet,
  useMobileTrackActions,
} from '../components/MobileActionSheets';
import MobileBottomSheet from '../components/MobileBottomSheet';
import MobileBoogieMixPanel from '../components/MobileBoogieMixPanel';

const SWIPE_ACTION_WIDTH = 112;
const SWIPE_DELETE_THRESHOLD = 92;
const REORDER_ROW_HEIGHT = 88;

type GestureMode = 'swipe' | 'reorder' | null;
type GestureSource = 'row' | 'handle';

type GestureState = {
  pointerId: number;
  mode: GestureMode;
  source: GestureSource;
  rowIndex: number;
  trackId: ClientEntityId;
  startX: number;
  startY: number;
  offsetX: number;
  targetIndex: number;
};
type PlaylistSortMode = 'manual' | 'artist' | 'album' | 'rating';

const PLAYLIST_SORT_LABELS: Record<PlaylistSortMode, string> = {
  manual: 'Manual order',
  artist: 'By artist',
  album: 'By album',
  rating: 'By rating',
};

export function fmtDuration(seconds: number | null | undefined): string {
  if (!seconds) return '0m';
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${Math.max(1, minutes)}m`;
}

export function fmtTrackDuration(seconds: number | null | undefined): string {
  if (!seconds) return '--';
  const minutes = Math.floor(seconds / 60);
  const remainder = Math.floor(seconds % 60);
  return `${minutes}:${String(remainder).padStart(2, '0')}`;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function buildCollageAlbumIds(tracks: PlaylistTrack[]): ClientEntityId[] {
  const ids = new Set<ClientEntityId>();
  for (const track of tracks) {
    if (!track.album_id || ids.has(track.album_id)) continue;
    ids.add(track.album_id);
    if (ids.size === 4) break;
  }
  return Array.from(ids);
}

export function updatePlaylistSummary(
  playlist: Playlist,
  trackDelta: number,
  durationDelta: number,
): Playlist {
  return {
    ...playlist,
    track_count: Math.max(0, (playlist.track_count ?? 0) + trackDelta),
    total_duration: Math.max(0, (playlist.total_duration ?? 0) + durationDelta),
  };
}

export function createFallbackTiles(count: number): number[] {
  return Array.from({ length: Math.max(0, 4 - count) }, (_, index) => index);
}

export function resolvePlaylistDuration(playlist: Playlist | null, tracks: PlaylistTrack[]): number {
  const summaryDuration = playlist?.total_duration ?? 0;
  if (summaryDuration > 0 || tracks.length === 0) return summaryDuration;
  return tracks.reduce((total, track) => total + (track.duration ?? 0), 0);
}

export function sortPlaylistTracks(tracks: PlaylistTrack[], sortMode: PlaylistSortMode): PlaylistTrack[] {
  if (sortMode === 'manual') return tracks;
  return [...tracks].sort((a, b) => {
    if (sortMode === 'rating') {
      const ratingDiff = (b.rating ?? -1) - (a.rating ?? -1);
      if (ratingDiff !== 0) return ratingDiff;
    }
    const left = sortMode === 'artist'
      ? [a.artist, a.album, a.title || a.file_name]
      : [a.album, a.artist, a.title || a.file_name];
    const right = sortMode === 'artist'
      ? [b.artist, b.album, b.title || b.file_name]
      : [b.album, b.artist, b.title || b.file_name];
    return left.map((value) => String(value ?? '').toLowerCase()).join('\u0000')
      .localeCompare(right.map((value) => String(value ?? '').toLowerCase()).join('\u0000'));
  });
}

function MobileAddTracksSheet({
  playlist,
  existingTrackIds,
  onClose,
  onAdded,
}: {
  playlist: Playlist;
  existingTrackIds: Set<ClientEntityId>;
  onClose: () => void;
  onAdded: () => void;
}) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<Track[]>([]);
  const [addingIds, setAddingIds] = useState<Set<ClientEntityId>>(new Set());
  const [addedIds, setAddedIds] = useState<Set<ClientEntityId>>(() => new Set(existingTrackIds));
  const [error, setError] = useState('');

  useEffect(() => {
    setAddedIds(new Set(existingTrackIds));
  }, [existingTrackIds]);

  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed) {
      setResults([]);
      setError('');
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      api.search({ q: trimmed, limit: 30, page: 1, search_mode: 'mobile_tracks' })
        .then((result) => {
          if (!cancelled) setResults(result.tracks ?? []);
        })
        .catch((err: any) => {
          if (!cancelled) setError(err?.message || 'Could not search tracks.');
        });
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query]);

  const addTrack = async (track: Track) => {
    if (addedIds.has(track.id)) return;
    setAddingIds((prev) => new Set(prev).add(track.id));
    setError('');
    try {
      await api.playlists.addTrack(playlist.id, track.id);
      setAddedIds((prev) => new Set(prev).add(track.id));
      onAdded();
    } catch (err: any) {
      setError(err?.message || 'Could not add track.');
    } finally {
      setAddingIds((prev) => {
        const next = new Set(prev);
        next.delete(track.id);
        return next;
      });
    }
  };

  return (
    <MobileBottomSheet title="Add Tracks" onClose={onClose}>
      <div style={styles.sheetStack}>
        <input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={`Search tracks for ${playlist.name}`}
          style={styles.searchInput}
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
        />
        {error ? <div role="alert" style={styles.error}>{error}</div> : null}
        {!query.trim() ? <div role="status" style={styles.emptyState}>Search your library to add tracks.</div> : null}
        {results.map((track) => {
          const isAdded = addedIds.has(track.id);
          const isAdding = addingIds.has(track.id);
          return (
            <div key={track.id} style={styles.addTrackRow}>
              <div style={styles.trackMeta}>
                <div style={styles.trackTitle}>{track.title || track.file_name}</div>
                <div style={styles.trackSub}>{[track.artist, track.album].filter(Boolean).join(' - ') || 'Unknown track'}</div>
              </div>
              {isAdded ? <span style={styles.duplicateBadge}>Already added</span> : null}
              <button
                type="button"
                style={styles.addTrackButton}
                disabled={isAdded || isAdding}
                onClick={() => void addTrack(track)}
              >
                {isAdding ? 'Adding...' : isAdded ? 'Added' : 'Add'}
              </button>
            </div>
          );
        })}
      </div>
    </MobileBottomSheet>
  );
}

/** Mobile Playlists View is part of this module's public API. */
export default function MobilePlaylistsView({
  initialPlaylistId,
  selection,
  onSelectionChange,
  onPlayTrack,
  onAddToQueue,
}: {
  initialPlaylistId: EntityId | null;
  selection: MobilePlaylistSelection;
  onSelectionChange: (selection: MobilePlaylistSelection) => void;
  onPlayTrack: (track: PlaylistTrack, allTracks?: PlaylistTrack[]) => void;
  onAddToQueue: (track: PlaylistTrack) => void;
}) {
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [gesture, setGesture] = useState<GestureState | null>(null);
  const [swipeOffsets, setSwipeOffsets] = useState<Record<string, number>>({});
  const [busyTrackId, setBusyTrackId] = useState<ClientEntityId | null>(null);
  const [error, setError] = useState('');
  const [editorMode, setEditorMode] = useState<'create' | 'edit'>('create');
  const [editorOpen, setEditorOpen] = useState(false);
  const [editorError, setEditorError] = useState('');
  const [editorSubmitting, setEditorSubmitting] = useState(false);
  const [trackFilter, setTrackFilter] = useState('');
  const [sortMode, setSortMode] = useState<PlaylistSortMode>('manual');
  const [sortOpen, setSortOpen] = useState(false);
  const [addTracksOpen, setAddTracksOpen] = useState(false);
  const pullStartYRef = useRef<number | null>(null);
  const suppressRowClickRef = useRef(false);
  const selectedPlaylistId = selection.playlist?.id ?? initialPlaylistId ?? null;

  const loadPlaylists = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const next = await api.playlists.list();
      setPlaylists(next);
    } catch (err: any) {
      setError(err?.message || 'Could not load playlists.');
    } finally {
      setLoading(false);
    }
  }, []);

  const loadSelectedPlaylist = useCallback(async (playlistId: EntityId) => {
    const [playlist, tracks] = await Promise.all([
      api.playlists.get(playlistId),
      api.playlists.tracks(playlistId),
    ]);
    onSelectionChange({ playlist, tracks });
    setPlaylists((prev) => prev.map((entry) => (entry.id === playlist.id ? playlist : entry)));
  }, [onSelectionChange]);

  useEffect(() => {
    void loadPlaylists();
  }, [loadPlaylists]);

  useEffect(() => {
    if (!selectedPlaylistId) return;
    let cancelled = false;
    Promise.all([
      api.playlists.get(selectedPlaylistId),
      api.playlists.tracks(selectedPlaylistId),
    ]).then(([playlist, tracks]) => {
      if (cancelled) return;
      onSelectionChange({ playlist, tracks });
      setPlaylists((prev) => prev.map((entry) => (entry.id === playlist.id ? playlist : entry)));
    }).catch(() => {
      if (!cancelled) setError('Could not load playlist.');
    });
    return () => {
      cancelled = true;
    };
  }, [onSelectionChange, selectedPlaylistId]);

  const playlistAlbumIds = useMemo(() => buildCollageAlbumIds(selection.tracks), [selection.tracks]);
  const playlistFallbackTiles = useMemo(() => createFallbackTiles(playlistAlbumIds.length), [playlistAlbumIds.length]);
  const playlistDuration = useMemo(
    () => resolvePlaylistDuration(selection.playlist, selection.tracks),
    [selection.playlist, selection.tracks],
  );
  const filteredTracks = useMemo(() => {
    const query = trackFilter.trim().toLowerCase();
    const visible = query
      ? selection.tracks.filter((track) => [
          track.title,
          track.file_name,
          track.artist,
          track.album,
        ].some((value) => String(value ?? '').toLowerCase().includes(query)))
      : selection.tracks;
    return sortPlaylistTracks(visible, sortMode);
  }, [selection.tracks, sortMode, trackFilter]);
  const existingTrackIds = useMemo(() => new Set(selection.tracks.map((track) => track.id)), [selection.tracks]);

  const persistReorder = useCallback(async (from: number, to: number) => {
    if (!selection.playlist || from === to || from < 0 || to < 0 || to >= selection.tracks.length) return;
    const nextTracks = [...selection.tracks];
    const [moved] = nextTracks.splice(from, 1);
    nextTracks.splice(to, 0, moved);
    onSelectionChange({ playlist: selection.playlist, tracks: nextTracks });
    setError('');
    try {
      await api.playlists.reorder(selection.playlist.id, nextTracks.map((track) => track.id));
    } catch {
      setError('Could not reorder tracks.');
      await loadSelectedPlaylist(selection.playlist.id).catch(() => {});
    }
  }, [loadSelectedPlaylist, onSelectionChange, selection.playlist, selection.tracks]);

  const removeTrack = useCallback(async (track: PlaylistTrack) => {
    if (!selection.playlist) return;
    if (track.playlist_track_id == null) {
      setError('Could not remove this track because its playlist row is missing an id.');
      return;
    }
    const nextPlaylist = updatePlaylistSummary(selection.playlist, -1, -(track.duration ?? 0));
    const nextTracks = selection.tracks.filter((entry) => entry.playlist_track_id !== track.playlist_track_id);
    setBusyTrackId(track.playlist_track_id);
    setSwipeOffsets((prev) => {
      const next = { ...prev };
      delete next[String(track.playlist_track_id)];
      return next;
    });
    onSelectionChange({ playlist: nextPlaylist, tracks: nextTracks });
    setPlaylists((prev) => prev.map((entry) => (entry.id === nextPlaylist.id ? nextPlaylist : entry)));
    setError('');
    try {
      await api.playlists.removeTrack(selection.playlist.id, track.playlist_track_id);
    } catch {
      setError('Could not remove track.');
      await loadSelectedPlaylist(selection.playlist.id).catch(() => {});
    } finally {
      setBusyTrackId(null);
    }
  }, [loadSelectedPlaylist, onSelectionChange, selection.playlist, selection.tracks]);

  const clearGesture = useCallback((trackId?: ClientEntityId) => {
    if (trackId != null) {
      const trackKey = String(trackId);
      setSwipeOffsets((prev) => {
        if (prev[trackKey] === undefined) return prev;
        const next = { ...prev };
        delete next[trackKey];
        return next;
      });
    }
    setGesture(null);
  }, []);

  const handlePointerDown = useCallback((
    event: React.PointerEvent<HTMLElement>,
    rowIndex: number,
    trackId: ClientEntityId,
    source: GestureSource,
  ) => {
    if (event.button !== 0) return;
    suppressRowClickRef.current = false;
    setGesture({
      pointerId: event.pointerId,
      mode: null,
      source,
      rowIndex,
      trackId,
      startX: event.clientX,
      startY: event.clientY,
      offsetX: swipeOffsets[String(trackId)] ?? 0,
      targetIndex: rowIndex,
    });
    const target = event.currentTarget as HTMLElement & {
      setPointerCapture?: (pointerId: number) => void;
    };
    target.setPointerCapture?.(event.pointerId);
  }, [swipeOffsets]);

  const handlePointerMove = useCallback((event: React.PointerEvent<HTMLElement>) => {
    setGesture((current) => {
      if (!current || current.pointerId !== event.pointerId) return current;
      const deltaX = event.clientX - current.startX;
      const deltaY = event.clientY - current.startY;
      if (current.source === 'handle') {
        const nextMode = current.mode ?? (Math.abs(deltaY) > 6 ? 'reorder' : null);
        if (nextMode !== 'reorder') return current;
        suppressRowClickRef.current = true;
        return {
          ...current,
          mode: 'reorder',
          targetIndex: clamp(current.rowIndex + Math.round(deltaY / REORDER_ROW_HEIGHT), 0, filteredTracks.length - 1),
        };
      }
      const nextMode = current.mode ?? (deltaX < -6 && Math.abs(deltaX) > Math.abs(deltaY) ? 'swipe' : null);
      if (nextMode !== 'swipe') return current;
      suppressRowClickRef.current = true;
      const nextOffset = clamp(deltaX, -SWIPE_ACTION_WIDTH, 0);
      setSwipeOffsets((prev) => ({ ...prev, [String(current.trackId)]: nextOffset }));
      return {
        ...current,
        mode: 'swipe',
        offsetX: nextOffset,
      };
    });
  }, [filteredTracks.length]);

  const handlePointerEnd = useCallback(async (event: React.PointerEvent<HTMLElement>) => {
    const current = gesture;
    if (!current || current.pointerId !== event.pointerId) return;
    const target = event.currentTarget as HTMLElement & {
      releasePointerCapture?: (pointerId: number) => void;
    };
    target.releasePointerCapture?.(event.pointerId);
    if (current.mode === 'swipe') {
      const track = selection.tracks.find((entry) => entry.playlist_track_id === current.trackId);
      if (!track) {
        clearGesture(current.trackId);
        return;
      }
      if ((swipeOffsets[String(current.trackId)] ?? 0) <= -SWIPE_DELETE_THRESHOLD) {
        suppressRowClickRef.current = true;
        clearGesture(current.trackId);
        await removeTrack(track);
        return;
      }
      clearGesture(current.trackId);
      return;
    }
    if (current.mode === 'reorder') {
      suppressRowClickRef.current = true;
      clearGesture(current.trackId);
      await persistReorder(current.rowIndex, current.targetIndex);
      return;
    }
    clearGesture(current.trackId);
  }, [clearGesture, gesture, persistReorder, removeTrack, selection.tracks, swipeOffsets]);

  const handleRowClick = useCallback((track: PlaylistTrack) => {
    if (suppressRowClickRef.current) {
      suppressRowClickRef.current = false;
      return;
    }
    onPlayTrack(track, selection.tracks);
  }, [onPlayTrack, selection.tracks]);

  const trackActions = useMobileTrackActions<PlaylistTrack>({
    playlists,
    onPlayTrack,
    onAddToQueue,
  });

  const openCreateEditor = useCallback(() => {
    setEditorMode('create');
    setEditorError('');
    setEditorOpen(true);
  }, []);

  const openEditEditor = useCallback(() => {
    setEditorMode('edit');
    setEditorError('');
    setEditorOpen(true);
  }, []);

  const refreshPlaylists = useCallback(async () => {
    setRefreshing(true);
    try {
      await loadPlaylists();
    } finally {
      setRefreshing(false);
    }
  }, [loadPlaylists]);

  const handleRootTouchStart = useCallback((event: React.TouchEvent<HTMLDivElement>) => {
    pullStartYRef.current = event.currentTarget.scrollTop <= 0 ? event.touches[0]?.clientY ?? null : null;
  }, []);

  const handleRootTouchMove = useCallback((event: React.TouchEvent<HTMLDivElement>) => {
    if (pullStartYRef.current == null || refreshing) return;
    const deltaY = (event.touches[0]?.clientY ?? pullStartYRef.current) - pullStartYRef.current;
    if (deltaY >= 72) {
      pullStartYRef.current = null;
      void refreshPlaylists();
    }
  }, [refreshPlaylists, refreshing]);

  const clearRootPull = useCallback(() => {
    pullStartYRef.current = null;
  }, []);

  const refreshSelectedPlaylist = useCallback(async () => {
    if (!selection.playlist) return;
    await loadSelectedPlaylist(selection.playlist.id);
  }, [loadSelectedPlaylist, selection.playlist]);

  const exportPlaylist = useCallback(() => {
    if (!selection.playlist) return;
    window.location.href = api.playlists.exportM3uUrl(selection.playlist.id);
  }, [selection.playlist]);

  const handleEditorSubmit = useCallback(async (name: string, description: string) => {
    setEditorSubmitting(true);
    setEditorError('');
    try {
      if (editorMode === 'create') {
        const created = await api.playlists.create(name, description);
        await loadPlaylists();
        onSelectionChange({ playlist: created, tracks: [] });
      } else if (selection.playlist) {
        const previousPlaylist = selection.playlist;
        const optimisticPlaylist = {
          ...previousPlaylist,
          name,
          description: description || null,
        };
        onSelectionChange({ playlist: optimisticPlaylist, tracks: selection.tracks });
        setPlaylists((prev) => prev.map((entry) => (entry.id === optimisticPlaylist.id ? optimisticPlaylist : entry)));
        try {
          const updated = await api.playlists.update(previousPlaylist.id, name, description);
          onSelectionChange({ playlist: updated, tracks: selection.tracks });
          setPlaylists((prev) => prev.map((entry) => (entry.id === updated.id ? updated : entry)));
        } catch (error) {
          onSelectionChange({ playlist: previousPlaylist, tracks: selection.tracks });
          setPlaylists((prev) => prev.map((entry) => (entry.id === previousPlaylist.id ? previousPlaylist : entry)));
          await loadSelectedPlaylist(previousPlaylist.id).catch(() => {});
          throw error;
        }
      }
      setEditorOpen(false);
    } catch (error: any) {
      setEditorError(error?.message || (editorMode === 'create' ? 'Could not create playlist.' : 'Could not update playlist.'));
    } finally {
      setEditorSubmitting(false);
    }
  }, [editorMode, loadPlaylists, loadSelectedPlaylist, onSelectionChange, selection.playlist, selection.tracks]);

  if (selection.playlist) {
    return (
      <main style={styles.page}>
        <div style={styles.headerBar}>
          <button
            type="button"
            style={styles.backBtn}
            aria-label="Back"
            onClick={() => {
              setError('');
              onSelectionChange({ playlist: null, tracks: [] });
            }}
          >
            Back
          </button>
          <div style={styles.headerMeta}>Playlist</div>
        </div>

        <section aria-labelledby="mobile-playlist-title" style={styles.heroCard}>
          <div style={styles.heroTopRow}>
            <div style={styles.collage}>
              {playlistAlbumIds.map((albumId) => (
                <div key={albumId} style={styles.collageTile}>
                  <ArtImage src={api.albumArtUrl(albumId, 300)} alt="" imgStyle={styles.collageArt} />
                </div>
              ))}
              {playlistFallbackTiles.map((tile) => (
                <div key={`fallback-${tile}`} style={{ ...styles.collageTile, ...styles.collageFallback }} />
              ))}
            </div>
            <div style={styles.heroTextBlock}>
              <h1 id="mobile-playlist-title" style={styles.heroTitle}>{selection.playlist.name}</h1>
              <div style={styles.heroSummary}>
                {selection.playlist.track_count} tracks - {fmtDuration(playlistDuration)}
              </div>
              {selection.playlist.description ? (
                <p style={styles.heroDescription}>{selection.playlist.description}</p>
              ) : null}
            </div>
          </div>
          <div style={styles.heroActions}>
            <button
              type="button"
              style={styles.heroGhostAction}
              onClick={() => setAddTracksOpen(true)}
            >
              Add
            </button>
            <button
              type="button"
              style={{
                ...styles.heroGhostAction,
                ...(!selection.tracks.length ? hybridMobileContentStyles.disabled : null),
              }}
              onClick={exportPlaylist}
              disabled={!selection.tracks.length}
            >
              Export
            </button>
            <button
              type="button"
              style={styles.heroGhostAction}
              onClick={openEditEditor}
            >
              Edit
            </button>
            <button
              type="button"
              style={{
                ...styles.heroPlay,
                ...(!selection.tracks.length ? hybridMobileContentStyles.disabled : null),
              }}
              onClick={() => selection.tracks[0] && onPlayTrack(selection.tracks[0], selection.tracks)}
              disabled={!selection.tracks.length}
            >
              Play
            </button>
          </div>
        </section>

        <MobileBoogieMixPanel
          key={String(selection.playlist.id)}
          playlistId={selection.playlist.id}
          playlistName={selection.playlist.name}
          trackCount={selection.tracks.length}
        />

        {error ? <div role="alert" style={styles.error}>{error}</div> : null}

        <div style={styles.trackListHeader}>
          <h2 style={styles.trackListHeading}>Tracks</h2>
          <button type="button" style={styles.sortButton} onClick={() => setSortOpen(true)}>
            {PLAYLIST_SORT_LABELS[sortMode]}
          </button>
        </div>
        <input
          type="search"
          value={trackFilter}
          onChange={(event) => setTrackFilter(event.target.value)}
          placeholder="Search this playlist"
          style={styles.playlistSearchInput}
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
        />
        <div style={styles.trackListHint}>
          {sortMode === 'manual' && !trackFilter.trim() ? 'Drag to reorder. Swipe left to delete.' : 'Sorted view. Swipe left to delete.'}
        </div>

        <div style={styles.trackList}>
          {filteredTracks.map((track, index) => {
            const trackGestureId = track.playlist_track_id ?? `${track.id}-${index}`;
            const swipeOffset = swipeOffsets[String(trackGestureId)] ?? 0;
            const isReordering = gesture?.mode === 'reorder' && gesture.trackId === trackGestureId;
            const isDropTarget = gesture?.mode === 'reorder' && gesture.targetIndex === index && gesture.rowIndex !== index;
            const canReorder = sortMode === 'manual' && !trackFilter.trim();
            return (
              <div key={trackGestureId} style={styles.trackShell}>
                <div style={styles.deleteAction}>Delete</div>
                <div
                  style={{
                    ...styles.trackRow,
                    transform: `translateX(${swipeOffset}px)`,
                    boxShadow: isReordering ? 'var(--shadow-raised)' : 'none',
                    borderColor: isDropTarget ? 'var(--accent)' : 'var(--divider-subtle)',
                    opacity: busyTrackId === track.playlist_track_id ? 0.55 : 1,
                  }}
                >
                  <button
                    type="button"
                    style={styles.trackButton}
                    aria-label={`Play ${track.title || track.file_name}`}
                    onPointerDown={(event) => handlePointerDown(event, index, trackGestureId, 'row')}
                    onPointerMove={handlePointerMove}
                    onPointerUp={(event) => void handlePointerEnd(event)}
                    onPointerCancel={(event) => void handlePointerEnd(event)}
                    onClick={() => handleRowClick(track)}
                  >
                    <div style={styles.trackThumbWrap}>
                      {track.album_id ? (
                        <ArtImage src={api.albumArtUrl(track.album_id, 300)} alt="" imgStyle={styles.trackThumb} />
                      ) : (
                        <div style={styles.trackThumbFallback} />
                      )}
                    </div>
                    <div style={styles.trackMeta}>
                      <div style={styles.trackTitle}>{track.title || track.file_name}</div>
                      <div style={styles.trackSub}>{[track.artist, track.album].filter(Boolean).join(' - ') || 'Unknown track'}</div>
                    </div>
                    <div style={styles.trackDuration}>{fmtTrackDuration(track.duration)}</div>
                  </button>
                  <button
                    type="button"
                    style={styles.kebabButton}
                    aria-label={`More actions for ${track.title || track.file_name}`}
                    onClick={(event) => {
                      event.stopPropagation();
                      suppressRowClickRef.current = true;
                      trackActions.openForTrack(track, selection.tracks);
                    }}
                  >
                    ...
                  </button>
                  <button
                    type="button"
                    style={{
                      ...styles.dragHandle,
                      ...(!canReorder ? hybridMobileContentStyles.disabled : null),
                    }}
                    aria-label={`Reorder ${track.title || track.file_name}`}
                    disabled={!canReorder}
                    onPointerDown={(event) => canReorder && handlePointerDown(event, index, trackGestureId, 'handle')}
                    onPointerMove={handlePointerMove}
                    onPointerUp={(event) => void handlePointerEnd(event)}
                    onPointerCancel={(event) => void handlePointerEnd(event)}
                    onClick={(event) => event.preventDefault()}
                  >
                    <span style={styles.dragDots} />
                    <span style={styles.dragDots} />
                    <span style={styles.dragDots} />
                  </button>
                </div>
              </div>
            );
          })}
          {!filteredTracks.length ? (
            <div role="status" style={styles.emptyState}>{selection.tracks.length ? 'No tracks match that search.' : 'This playlist is empty.'}</div>
          ) : null}
        </div>
        {sortOpen ? (
          <MobileBottomSheet title="Sort Playlist" onClose={() => setSortOpen(false)}>
            <div style={styles.sheetStack}>
              {(Object.keys(PLAYLIST_SORT_LABELS) as PlaylistSortMode[]).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  style={{ ...styles.sheetAction, ...(sortMode === mode ? styles.sheetActionActive : null) }}
                  aria-pressed={sortMode === mode}
                  onClick={() => {
                    setSortMode(mode);
                    setSortOpen(false);
                  }}
                >
                  {PLAYLIST_SORT_LABELS[mode]}
                </button>
              ))}
            </div>
          </MobileBottomSheet>
        ) : null}
        {addTracksOpen ? (
          <MobileAddTracksSheet
            playlist={selection.playlist}
            existingTrackIds={existingTrackIds}
            onClose={() => setAddTracksOpen(false)}
            onAdded={() => void refreshSelectedPlaylist()}
          />
        ) : null}
        {trackActions.actionsSheet}
        {trackActions.pickerSheet}
        {trackActions.createSheet}
        <MobilePlaylistEditorSheet
          open={editorOpen}
          mode="edit"
          existingPlaylists={playlists}
          initialName={selection.playlist.name}
          initialDescription={selection.playlist.description ?? ''}
          error={editorError}
          submitting={editorSubmitting}
          onClose={() => {
            setEditorError('');
            setEditorOpen(false);
          }}
          onSubmit={handleEditorSubmit}
        />
      </main>
    );
  }

  return (
    <main
      style={{ ...styles.page, overscrollBehaviorY: 'contain' }}
      aria-busy={loading || refreshing}
      onTouchStart={handleRootTouchStart}
      onTouchMove={handleRootTouchMove}
      onTouchEnd={clearRootPull}
      onTouchCancel={clearRootPull}
    >
      <header style={hybridMobileContentStyles.pageHeader}>
        <div style={styles.listKicker}>Library</div>
        <h1 style={styles.listHeader}>Playlists</h1>
        <p style={styles.listSubhead}>Build, edit, and jump into playlists from your phone.</p>
        <button type="button" style={styles.newPlaylistButton} onClick={openCreateEditor}>New Playlist</button>
      </header>
      {refreshing ? <div role="status" style={styles.refreshState}>Refreshing playlists...</div> : null}
      {error ? (
        <div role="alert" style={styles.error}>
          {error}
          <button type="button" style={styles.retryButton} onClick={() => void loadPlaylists()}>Retry</button>
        </div>
      ) : null}
      {loading ? <div role="status" style={styles.loadingState}>Loading playlists...</div> : null}
      {!error && !loading && !playlists.length ? (
        <div role="status" style={styles.emptyCreateState}>
          <div style={styles.emptyCreateTitle}>No playlists yet.</div>
          <div style={styles.emptyCreateCopy}>Start one here, then use track kebabs across Browse, Search, and playlists to fill it up fast.</div>
          <button type="button" style={styles.emptyCreateButton} onClick={openCreateEditor}>Create Playlist</button>
        </div>
      ) : null}
      {!loading ? (
        <div style={styles.playlistList}>
          {playlists.map((playlist) => (
            <button
              key={playlist.id}
              type="button"
              style={styles.card}
              onClick={() => onSelectionChange({ playlist, tracks: [] })}
            >
              <span style={styles.cardArtwork}>
                {playlist.art_album_ids?.[0] ? (
                  <ArtImage
                    src={api.albumArtUrl(playlist.art_album_ids[0], 300)}
                    alt=""
                    imgStyle={hybridMobileContentStyles.listArtworkImage}
                  />
                ) : (
                  <span style={hybridMobileContentStyles.listArtworkFallback}>≡</span>
                )}
              </span>
              <span style={styles.cardMeta}>
                <span style={styles.cardTitle}>{playlist.name}</span>
                <span style={styles.cardSub}>{playlist.track_count} tracks - {fmtDuration(playlist.total_duration)}</span>
              </span>
              <span style={hybridMobileContentStyles.listBadge}>Open</span>
            </button>
          ))}
        </div>
      ) : null}
      <MobilePlaylistEditorSheet
        open={editorOpen}
        mode="create"
        existingPlaylists={playlists}
        error={editorError}
        submitting={editorSubmitting}
        onClose={() => {
          setEditorError('');
          setEditorOpen(false);
        }}
        onSubmit={handleEditorSubmit}
      />
    </main>
  );
}

const styles: Record<string, React.CSSProperties> = {
  page: phase2.mobilePage,
  headerBar: {
    minHeight: 44,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    marginBottom: 12,
  },
  backBtn: {
    ...hybridMobileContentStyles.secondaryAction,
    minHeight: 44,
    padding: '0 14px',
  },
  headerMeta: hybridMobileContentStyles.eyebrow,
  heroCard: {
    display: 'grid',
    gap: 14,
    padding: 12,
    marginBottom: 18,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 18,
    background: 'var(--surface)',
  },
  heroTopRow: {
    display: 'grid',
    gridTemplateColumns: '104px minmax(0, 1fr)',
    alignItems: 'center',
    gap: 14,
  },
  collage: {
    width: 104,
    aspectRatio: '1 / 1',
    display: 'grid',
    gridTemplateColumns: 'repeat(2, 1fr)',
    gap: 2,
    borderRadius: 16,
    overflow: 'hidden',
    flexShrink: 0,
    boxShadow: 'var(--shadow-subtle)',
    background: 'var(--surface-subtle)',
  },
  collageTile: { minWidth: 0, minHeight: 0, background: 'var(--surface-subtle)' },
  collageArt: { width: '100%', height: '100%', objectFit: 'cover', display: 'block' },
  collageFallback: { background: 'linear-gradient(135deg, var(--accent-soft), var(--surface-subtle))' },
  heroActions: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
    gap: 8,
  },
  heroGhostAction: {
    minWidth: 0,
    minHeight: 44,
    padding: '0 12px',
    borderRadius: 11,
    border: '1px solid var(--divider-subtle)',
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 11,
    fontWeight: 750,
  },
  heroPlay: {
    minWidth: 0,
    minHeight: 44,
    padding: '0 14px',
    borderRadius: 11,
    border: 'none',
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    fontFamily: 'inherit',
    fontSize: 11,
    fontWeight: 750,
  },
  heroTextBlock: { minWidth: 0 },
  heroTitle: hybridMobileContentStyles.detailTitle,
  heroSummary: hybridMobileContentStyles.detailMeta,
  heroDescription: {
    margin: '7px 0 0',
    color: 'var(--text-muted)',
    fontSize: 10,
    lineHeight: 1.45,
  },
  error: {
    ...hybridMobileContentStyles.feedback,
    ...hybridMobileContentStyles.feedbackError,
    marginBottom: 12,
    display: 'grid',
    gap: 8,
  },
  retryButton: {
    minHeight: 44,
    borderRadius: 11,
    border: '1px solid color-mix(in srgb, var(--danger) 34%, var(--divider-subtle))',
    background: 'var(--surface)',
    color: 'var(--danger)',
    fontFamily: 'inherit',
    fontSize: 11,
    fontWeight: 750,
  },
  trackListHeader: hybridMobileContentStyles.sectionHeader,
  trackListHeading: hybridMobileContentStyles.sectionTitle,
  trackListHint: {
    color: 'var(--text-muted)',
    fontSize: 10,
    lineHeight: 1.45,
    marginBottom: 10,
  },
  sortButton: {
    ...hybridMobileContentStyles.chip,
    minHeight: 44,
  },
  playlistSearchInput: {
    ...hybridMobileContentStyles.field,
    marginBottom: 8,
  },
  searchInput: hybridMobileContentStyles.field,
  trackList: hybridMobileContentStyles.list,
  trackShell: { position: 'relative', overflow: 'hidden', borderRadius: 14 },
  deleteAction: {
    position: 'absolute',
    inset: 0,
    display: 'flex',
    justifyContent: 'flex-end',
    alignItems: 'center',
    paddingRight: 18,
    background: 'linear-gradient(90deg, color-mix(in srgb, var(--danger) 8%, var(--surface)), var(--danger))',
    color: 'var(--on-accent)',
    fontSize: 11,
    fontWeight: 800,
    letterSpacing: 0.4,
  },
  trackRow: {
    position: 'relative',
    minHeight: 68,
    display: 'flex',
    alignItems: 'stretch',
    overflow: 'hidden',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface)',
    transition: 'transform 160ms ease, box-shadow 160ms ease, border-color 160ms ease, opacity 160ms ease',
    touchAction: 'pan-y',
  },
  trackButton: {
    flex: 1,
    minWidth: 0,
    display: 'flex',
    alignItems: 'center',
    gap: 10,
    padding: '8px 10px',
    border: 'none',
    background: 'transparent',
    color: 'var(--text)',
    textAlign: 'left',
    fontFamily: 'inherit',
    touchAction: 'pan-y',
  },
  trackThumbWrap: {
    width: 48,
    height: 48,
    borderRadius: 11,
    overflow: 'hidden',
    flexShrink: 0,
    background: 'var(--surface-subtle)',
  },
  trackThumb: { width: '100%', height: '100%', objectFit: 'cover', display: 'block' },
  trackThumbFallback: { width: '100%', height: '100%', background: 'linear-gradient(135deg, var(--accent-soft), var(--surface-subtle))' },
  trackMeta: { minWidth: 0, display: 'grid', gap: 3, flex: 1 },
  trackTitle: {
    color: 'var(--text)',
    fontSize: 13,
    fontWeight: 750,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  trackSub: {
    color: 'var(--text-muted)',
    fontSize: 10,
    fontWeight: 600,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  trackDuration: {
    color: 'var(--text-muted)',
    fontSize: 10,
    fontWeight: 650,
    alignSelf: 'center',
    flexShrink: 0,
    marginLeft: 8,
  },
  kebabButton: {
    width: 44,
    minWidth: 44,
    display: 'grid',
    placeItems: 'center',
    border: 'none',
    borderLeft: '1px solid var(--divider-subtle)',
    background: 'transparent',
    color: 'var(--text-muted)',
    padding: 0,
    fontFamily: 'inherit',
    fontSize: 18,
    lineHeight: 1,
    letterSpacing: 1,
  },
  dragHandle: {
    width: 44,
    display: 'grid',
    placeItems: 'center',
    border: 'none',
    borderLeft: '1px solid var(--divider-subtle)',
    borderRadius: 0,
    background: 'transparent',
    color: 'var(--text-muted)',
    padding: 0,
    touchAction: 'none',
  },
  sheetStack: { display: 'grid', gap: 10 },
  sheetAction: {
    minHeight: 52,
    borderRadius: 12,
    border: '1px solid var(--divider-subtle)',
    background: 'var(--surface)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 750,
    textAlign: 'left',
    padding: '0 14px',
  },
  sheetActionActive: {
    borderColor: 'color-mix(in srgb, var(--accent) 34%, var(--divider-subtle))',
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
  },
  addTrackRow: {
    display: 'grid',
    gridTemplateColumns: 'minmax(0, 1fr) auto auto',
    alignItems: 'center',
    gap: 8,
    padding: '10px 0',
    borderBottom: '1px solid color-mix(in srgb, var(--border) 72%, transparent)',
  },
  duplicateBadge: {
    padding: '5px 8px',
    borderRadius: 999,
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    fontSize: 11,
    fontWeight: 800,
    whiteSpace: 'nowrap',
  },
  addTrackButton: {
    minHeight: 44,
    minWidth: 64,
    borderRadius: 14,
    border: '1px solid var(--border)',
    background: 'var(--surface)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 800,
  },
  dragDots: {
    width: 4,
    height: 4,
    borderRadius: 999,
    background: 'currentColor',
    boxShadow: '0 8px 0 currentColor, 0 -8px 0 currentColor',
  },
  emptyState: {
    ...hybridMobileContentStyles.feedback,
    textAlign: 'left',
  },
  listKicker: hybridMobileContentStyles.eyebrow,
  listHeader: hybridMobileContentStyles.pageTitle,
  listSubhead: { ...hybridMobileContentStyles.pageBody, marginBottom: 4 },
  refreshState: {
    ...hybridMobileContentStyles.feedback,
    marginTop: 14,
    color: 'var(--accent)',
  },
  loadingState: {
    ...hybridMobileContentStyles.feedback,
    marginTop: 14,
  },
  newPlaylistButton: {
    minHeight: 48,
    width: '100%',
    marginTop: 4,
    borderRadius: 12,
    border: 'none',
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 750,
  },
  emptyCreateState: {
    ...hybridMobileContentStyles.empty,
    display: 'grid',
    gap: 10,
    marginTop: 14,
  },
  emptyCreateTitle: hybridMobileContentStyles.emptyTitle,
  emptyCreateCopy: hybridMobileContentStyles.emptyBody,
  emptyCreateButton: {
    minHeight: 44,
    borderRadius: 11,
    border: '1px solid var(--divider-subtle)',
    background: 'var(--surface)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 11,
    fontWeight: 750,
  },
  playlistList: { ...hybridMobileContentStyles.list, marginTop: 14 },
  card: hybridMobileContentStyles.listRow,
  cardArtwork: hybridMobileContentStyles.listArtwork,
  cardMeta: hybridMobileContentStyles.listMeta,
  cardTitle: hybridMobileContentStyles.listTitle,
  cardSub: hybridMobileContentStyles.listSubtitle,
};

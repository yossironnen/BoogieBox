/**
 * Defines mobile Mobile Action Sheets behavior for the BoogieBox React client.
 */

import React, { useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../../api';
import ArtImage from '../../components/ArtImage';
import { hybridMobileContentStyles } from '../../hybridPreview';
import type { Playlist, PlaylistTrack, Track } from '../../types';
import type { EntityId } from '../../entityId';
import MobileBottomSheet from './MobileBottomSheet';

type TrackLike = Track | PlaylistTrack;

function MobileTrackSummary({ track }: { track: TrackLike | null }) {
  return (
    <div style={styles.trackSummary}>
      <span aria-hidden="true" style={styles.trackArtwork}>
        {track?.album_id ? (
          <ArtImage
            src={api.albumArtUrl(track.album_id, 300)}
            alt=""
            imgStyle={styles.trackArtworkImage}
          />
        ) : (
          <span style={styles.trackArtworkFallback}>♪</span>
        )}
      </span>
      <span style={styles.trackSummaryMeta}>
        <span style={styles.pickerTrackTitle}>{track?.title || track?.file_name || 'Track'}</span>
        <span style={styles.pickerTrackMeta}>{[track?.artist, track?.album].filter(Boolean).join(' - ') || 'Quick actions'}</span>
      </span>
    </div>
  );
}

/** Normalize Playlist Name is part of this module's public API. */
export function normalizePlaylistName(name: string): string {
  return name.trim().replace(/\s+/g, ' ').toLowerCase();
}

function MobileSheet({
  open,
  onClose,
  title,
  children,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: React.ReactNode;
}) {
  if (!open) return null;

  return (
    <MobileBottomSheet title={title} onClose={onClose} closeLabel={`Close ${title}`}>
      {children}
    </MobileBottomSheet>
  );
}

/** Mobile Playlist Editor Sheet is part of this module's public API. */
export function MobilePlaylistEditorSheet({
  open,
  mode,
  existingPlaylists,
  initialName = '',
  initialDescription = '',
  error,
  submitting,
  onClose,
  onSubmit,
}: {
  open: boolean;
  mode: 'create' | 'edit';
  existingPlaylists: Playlist[];
  initialName?: string;
  initialDescription?: string;
  error?: string;
  submitting?: boolean;
  onClose: () => void;
  onSubmit: (name: string, description: string) => Promise<unknown> | unknown;
}) {
  const [name, setName] = useState(initialName);
  const [description, setDescription] = useState(initialDescription);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!open) return;
    setName(initialName);
    setDescription(initialDescription);
  }, [initialDescription, initialName, open]);

  useEffect(() => {
    if (!open) return undefined;
    const timer = window.setTimeout(() => inputRef.current?.focus(), 30);
    return () => window.clearTimeout(timer);
  }, [open]);

  const normalizedCurrent = normalizePlaylistName(initialName);
  const duplicate = useMemo(() => {
    const normalized = normalizePlaylistName(name);
    if (!normalized) return false;
    return existingPlaylists.some((playlist) => {
      const candidate = normalizePlaylistName(playlist.name);
      if (mode === 'edit' && candidate === normalizedCurrent) return false;
      return candidate === normalized;
    });
  }, [existingPlaylists, mode, name, normalizedCurrent]);

  const canSubmit = name.trim().length > 0 && !duplicate && !submitting;

  return (
    <MobileSheet open={open} onClose={onClose} title={mode === 'create' ? 'New Playlist' : 'Edit Playlist'}>
      <div style={styles.form}>
        <label style={styles.label}>
          <span style={styles.labelText}>Name</span>
          <input
            ref={inputRef}
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="Late-night mix"
            style={styles.input}
            aria-invalid={duplicate || Boolean(error)}
          />
        </label>
        <label style={styles.label}>
          <span style={styles.labelText}>Description</span>
          <textarea
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            placeholder="Optional notes"
            style={styles.textarea}
            rows={4}
          />
        </label>
        {duplicate ? <div role="alert" style={styles.errorText}>A playlist with this name already exists.</div> : null}
        {error ? <div role="alert" style={styles.errorText}>{error}</div> : null}
        <button
          type="button"
          style={{ ...styles.primaryButton, ...(canSubmit ? null : styles.primaryButtonDisabled) }}
          disabled={!canSubmit}
          onClick={() => void onSubmit(name.trim(), description.trim())}
        >
          {submitting ? (mode === 'create' ? 'Creating...' : 'Saving...') : (mode === 'create' ? 'Create playlist' : 'Save changes')}
        </button>
      </div>
    </MobileSheet>
  );
}

/** Mobile Playlist Picker Sheet is part of this module's public API. */
export function MobilePlaylistPickerSheet({
  open,
  playlists,
  track,
  error,
  busyPlaylistId,
  onClose,
  onPickPlaylist,
  onCreatePlaylist,
}: {
  open: boolean;
  playlists: Playlist[];
  track: TrackLike | null;
  error?: string;
  busyPlaylistId?: EntityId | null;
  onClose: () => void;
  onPickPlaylist: (playlist: Playlist) => Promise<unknown> | unknown;
  onCreatePlaylist: () => void;
}) {
  return (
    <MobileSheet open={open} onClose={onClose} title="Add To Playlist">
      <MobileTrackSummary track={track} />
      <button type="button" style={styles.secondaryButton} onClick={onCreatePlaylist}>
        New Playlist
      </button>
      <div style={styles.list}>
        {playlists.map((playlist) => (
          <button
            key={playlist.id}
            type="button"
            style={{
              ...styles.listRow,
              ...(busyPlaylistId === playlist.id ? hybridMobileContentStyles.disabled : null),
            }}
            onClick={() => void onPickPlaylist(playlist)}
            disabled={busyPlaylistId === playlist.id}
            aria-label={playlist.name}
          >
            <span aria-hidden="true" style={styles.listArtwork}>
              {playlist.art_album_ids?.[0] ? (
                <ArtImage
                  src={api.albumArtUrl(playlist.art_album_ids[0], 300)}
                  alt=""
                  imgStyle={styles.listArtworkImage}
                />
              ) : (
                <span style={styles.listArtworkFallback}>≡</span>
              )}
            </span>
            <span style={styles.listRowMain}>
              <span style={styles.listRowTitle}>{playlist.name}</span>
              <span style={styles.listRowMeta}>{playlist.track_count} tracks</span>
            </span>
            <span style={styles.listRowStatus}>{busyPlaylistId === playlist.id ? 'Adding...' : 'Add'}</span>
          </button>
        ))}
        {!playlists.length ? <div role="status" style={styles.emptyText}>No playlists yet. Create one to keep this track.</div> : null}
      </div>
      {error ? <div role="alert" style={styles.errorText}>{error}</div> : null}
    </MobileSheet>
  );
}

/** Mobile Track Actions Sheet is part of this module's public API. */
export function MobileTrackActionsSheet({
  open,
  track,
  onClose,
  onPlayNow,
  onAddToQueue,
  onAddToPlaylist,
}: {
  open: boolean;
  track: TrackLike | null;
  onClose: () => void;
  onPlayNow: () => void;
  onAddToQueue: () => void;
  onAddToPlaylist: () => void;
}) {
  return (
    <MobileSheet open={open} onClose={onClose} title="Track Actions">
      <MobileTrackSummary track={track} />
      <div style={styles.actionsList}>
        <button type="button" aria-label="Play Now" style={styles.actionButton} onClick={onPlayNow}>
          <span style={styles.actionTitle}>Play Now</span>
          <span style={styles.actionMeta}>Start this track immediately</span>
        </button>
        <button type="button" aria-label="Add To Queue" style={styles.actionButton} onClick={onAddToQueue}>
          <span style={styles.actionTitle}>Add To Queue</span>
          <span style={styles.actionMeta}>Keep it in the current session</span>
        </button>
        <button type="button" aria-label="Add To Playlist" style={styles.actionButton} onClick={onAddToPlaylist}>
          <span style={styles.actionTitle}>Add To Playlist</span>
          <span style={styles.actionMeta}>Save it for later</span>
        </button>
      </div>
    </MobileSheet>
  );
}

/** Use Mobile Track Actions is part of this module's public API. */
export function useMobileTrackActions<TTrack extends TrackLike>({
  playlists,
  onPlayTrack,
  onAddToQueue,
}: {
  playlists: Playlist[];
  onPlayTrack: (track: TTrack, allTracks?: TTrack[]) => void;
  onAddToQueue: (track: TTrack) => void;
}) {
  const [activeTrack, setActiveTrack] = useState<TTrack | null>(null);
  const [activeQueue, setActiveQueue] = useState<TTrack[] | undefined>(undefined);
  const [actionsOpen, setActionsOpen] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerError, setPickerError] = useState('');
  const [busyPlaylistId, setBusyPlaylistId] = useState<EntityId | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createError, setCreateError] = useState('');
  const [createSubmitting, setCreateSubmitting] = useState(false);

  const openForTrack = (track: TTrack, allTracks?: TTrack[]) => {
    setActiveTrack(track);
    setActiveQueue(allTracks);
    setPickerError('');
    setCreateError('');
    setActionsOpen(true);
    setPickerOpen(false);
    setCreateOpen(false);
  };

  const closeAll = () => {
    setActionsOpen(false);
    setPickerOpen(false);
    setCreateOpen(false);
    setBusyPlaylistId(null);
    setPickerError('');
    setCreateError('');
  };

  const addTrackToPlaylist = async (playlist: Playlist) => {
    if (!activeTrack) return;
    setBusyPlaylistId(playlist.id);
    setPickerError('');
    try {
      await api.playlists.addTrack(playlist.id, activeTrack.id);
      closeAll();
    } catch (error: any) {
      setPickerError(error?.message || 'Could not add track to playlist.');
    } finally {
      setBusyPlaylistId(null);
    }
  };

  const createPlaylistAndAdd = async (name: string, description: string) => {
    if (!activeTrack) return null;
    setCreateSubmitting(true);
    setCreateError('');
    try {
      const playlist = await api.playlists.create(name, description);
      await api.playlists.addTrack(playlist.id, activeTrack.id);
      closeAll();
      return playlist;
    } catch (error: any) {
      setCreateError(error?.message || 'Could not create playlist.');
      return null;
    } finally {
      setCreateSubmitting(false);
    }
  };

  return {
    openForTrack,
    closeAll,
    actionsSheet: (
      <MobileTrackActionsSheet
        open={actionsOpen}
        track={activeTrack}
        onClose={closeAll}
        onPlayNow={() => {
          if (!activeTrack) return;
          closeAll();
          onPlayTrack(activeTrack, activeQueue);
        }}
        onAddToQueue={() => {
          if (!activeTrack) return;
          closeAll();
          onAddToQueue(activeTrack);
        }}
        onAddToPlaylist={() => {
          setActionsOpen(false);
          setPickerOpen(true);
        }}
      />
    ),
    pickerSheet: (
      <MobilePlaylistPickerSheet
        open={pickerOpen}
        playlists={playlists}
        track={activeTrack}
        error={pickerError}
        busyPlaylistId={busyPlaylistId}
        onClose={closeAll}
        onPickPlaylist={addTrackToPlaylist}
        onCreatePlaylist={() => {
          setPickerOpen(false);
          setCreateOpen(true);
        }}
      />
    ),
    createSheet: (
      <MobilePlaylistEditorSheet
        open={createOpen}
        mode="create"
        existingPlaylists={playlists}
        error={createError}
        submitting={createSubmitting}
        onClose={closeAll}
        onSubmit={createPlaylistAndAdd}
      />
    ),
  };
}

const styles: Record<string, React.CSSProperties> = {
  form: {
    display: 'grid',
    gap: 12,
  },
  label: {
    display: 'grid',
    gap: 8,
  },
  labelText: {
    color: 'var(--text-muted)',
    fontSize: 10,
    fontWeight: 750,
    textTransform: 'uppercase',
    letterSpacing: 1,
  },
  input: {
    width: '100%',
    minHeight: 52,
    boxSizing: 'border-box',
    padding: '0 15px',
    borderRadius: 14,
    border: '1px solid var(--border)',
    background: 'var(--surface)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 15,
    outline: 'none',
  },
  textarea: {
    width: '100%',
    resize: 'vertical',
    minHeight: 112,
    boxSizing: 'border-box',
    padding: '14px 16px',
    borderRadius: 14,
    border: '1px solid var(--border)',
    background: 'var(--surface)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 15,
    outline: 'none',
  },
  primaryButton: {
    minHeight: 52,
    borderRadius: 12,
    border: 'none',
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 750,
  },
  primaryButtonDisabled: hybridMobileContentStyles.disabled,
  secondaryButton: {
    minHeight: 44,
    borderRadius: 11,
    border: '1px solid var(--divider-subtle)',
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 11,
    fontWeight: 750,
  },
  errorText: {
    ...hybridMobileContentStyles.feedback,
    ...hybridMobileContentStyles.feedbackError,
  },
  trackSummary: {
    minHeight: 66,
    display: 'grid',
    gridTemplateColumns: '48px minmax(0, 1fr)',
    alignItems: 'center',
    gap: 10,
    boxSizing: 'border-box',
    padding: '8px 10px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface-subtle)',
  },
  trackArtwork: {
    width: 48,
    height: 48,
    overflow: 'hidden',
    borderRadius: 11,
    background: 'var(--surface)',
  },
  trackArtworkImage: {
    width: '100%',
    height: '100%',
    display: 'block',
    objectFit: 'cover',
  },
  trackArtworkFallback: {
    width: '100%',
    height: '100%',
    display: 'grid',
    placeItems: 'center',
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    fontSize: 18,
    fontWeight: 800,
  },
  trackSummaryMeta: {
    minWidth: 0,
    display: 'grid',
    gap: 3,
  },
  pickerTrackTitle: {
    overflow: 'hidden',
    color: 'var(--text)',
    fontSize: 13,
    fontWeight: 750,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  pickerTrackMeta: {
    color: 'var(--text-muted)',
    fontSize: 10,
    fontWeight: 600,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  list: {
    display: 'grid',
    gap: 6,
  },
  listRow: {
    minHeight: 66,
    display: 'grid',
    gridTemplateColumns: '48px minmax(0, 1fr) auto',
    alignItems: 'center',
    gap: 10,
    boxSizing: 'border-box',
    padding: '8px 9px',
    borderRadius: 14,
    border: '1px solid var(--divider-subtle)',
    background: 'var(--surface)',
    color: 'var(--text)',
    textAlign: 'left',
    fontFamily: 'inherit',
  },
  listArtwork: {
    width: 48,
    height: 48,
    overflow: 'hidden',
    borderRadius: 11,
    background: 'var(--surface-subtle)',
  },
  listArtworkImage: {
    width: '100%',
    height: '100%',
    display: 'block',
    objectFit: 'cover',
  },
  listArtworkFallback: {
    width: '100%',
    height: '100%',
    display: 'grid',
    placeItems: 'center',
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    fontSize: 17,
    fontWeight: 800,
  },
  listRowMain: {
    minWidth: 0,
    display: 'grid',
    gap: 3,
  },
  listRowTitle: {
    overflow: 'hidden',
    color: 'var(--text)',
    fontSize: 13,
    fontWeight: 750,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  listRowMeta: {
    color: 'var(--text-muted)',
    fontSize: 10,
    fontWeight: 600,
  },
  listRowStatus: {
    color: 'var(--accent)',
    flexShrink: 0,
    fontSize: 10,
    fontWeight: 750,
  },
  emptyText: {
    ...hybridMobileContentStyles.feedback,
  },
  actionsList: {
    display: 'grid',
    gap: 10,
  },
  actionButton: {
    minHeight: 58,
    display: 'grid',
    gap: 3,
    padding: '10px 12px',
    borderRadius: 12,
    border: '1px solid var(--divider-subtle)',
    background: 'var(--surface)',
    color: 'var(--text)',
    textAlign: 'left',
    fontFamily: 'inherit',
  },
  actionTitle: {
    color: 'var(--text)',
    fontSize: 12,
    fontWeight: 750,
  },
  actionMeta: {
    color: 'var(--text-muted)',
    fontSize: 10,
    fontWeight: 600,
  },
};

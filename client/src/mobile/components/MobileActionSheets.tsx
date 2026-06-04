/**
 * Defines mobile Mobile Action Sheets behavior for the BoogieBox React client.
 */

import React, { useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../../api';
import type { Playlist, PlaylistTrack, Track } from '../../types';
import type { EntityId } from '../../entityId';

type TrackLike = Track | PlaylistTrack;

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
  useEffect(() => {
    if (!open) return undefined;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose, open]);

  if (!open) return null;

  return (
    <div style={styles.overlay} onClick={onClose}>
      <div style={styles.sheet} onClick={(event) => event.stopPropagation()}>
        <div style={styles.grabber} />
        <div style={styles.sheetHeader}>
          <div style={styles.sheetTitle}>{title}</div>
          <button type="button" style={styles.closeButton} onClick={onClose} aria-label={`Close ${title}`}>
            Close
          </button>
        </div>
        <div style={styles.sheetBody}>{children}</div>
      </div>
    </div>
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
        {duplicate ? <div style={styles.errorText}>A playlist with this name already exists.</div> : null}
        {error ? <div style={styles.errorText}>{error}</div> : null}
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
      <div style={styles.pickerHeader}>
        <div style={styles.pickerTrackTitle}>{track?.title || track?.file_name || 'Track'}</div>
        <div style={styles.pickerTrackMeta}>{[track?.artist, track?.album].filter(Boolean).join(' - ') || 'Choose a playlist'}</div>
      </div>
      <button type="button" style={styles.secondaryButton} onClick={onCreatePlaylist}>
        New Playlist
      </button>
      <div style={styles.list}>
        {playlists.map((playlist) => (
          <button
            key={playlist.id}
            type="button"
            style={styles.listRow}
            onClick={() => void onPickPlaylist(playlist)}
            disabled={busyPlaylistId === playlist.id}
            aria-label={playlist.name}
          >
            <span style={styles.listRowMain}>
              <span style={styles.listRowTitle}>{playlist.name}</span>
              <span style={styles.listRowMeta}>{playlist.track_count} tracks</span>
            </span>
            <span style={styles.listRowStatus}>{busyPlaylistId === playlist.id ? 'Adding...' : 'Add'}</span>
          </button>
        ))}
        {!playlists.length ? <div style={styles.emptyText}>No playlists yet. Create one to keep this track.</div> : null}
      </div>
      {error ? <div style={styles.errorText}>{error}</div> : null}
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
      <div style={styles.pickerHeader}>
        <div style={styles.pickerTrackTitle}>{track?.title || track?.file_name || 'Track'}</div>
        <div style={styles.pickerTrackMeta}>{[track?.artist, track?.album].filter(Boolean).join(' - ') || 'Quick actions'}</div>
      </div>
      <div style={styles.actionsList}>
        <button type="button" style={styles.actionButton} onClick={onPlayNow}>Play Now</button>
        <button type="button" style={styles.actionButton} onClick={onAddToQueue}>Add To Queue</button>
        <button type="button" style={styles.actionButton} onClick={onAddToPlaylist}>Add To Playlist</button>
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
  overlay: {
    position: 'fixed',
    inset: 0,
    background: 'rgba(0,0,0,0.56)',
    display: 'flex',
    alignItems: 'flex-end',
    justifyContent: 'stretch',
    zIndex: 1200,
  },
  sheet: {
    width: '100%',
    borderTopLeftRadius: 28,
    borderTopRightRadius: 28,
    background: 'linear-gradient(180deg, color-mix(in srgb, var(--surface) 96%, var(--bg)) 0%, color-mix(in srgb, var(--surface) 88%, var(--bg)) 100%)',
    borderTop: '1px solid color-mix(in srgb, var(--border) 76%, transparent)',
    padding: '10px 16px calc(20px + env(safe-area-inset-bottom))',
    maxHeight: '82dvh',
    boxShadow: '0 -18px 40px rgba(0,0,0,0.32)',
  },
  grabber: {
    width: 44,
    height: 5,
    borderRadius: 999,
    background: 'rgba(255,255,255,0.18)',
    margin: '0 auto 14px',
  },
  sheetHeader: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    marginBottom: 14,
  },
  sheetTitle: {
    color: 'var(--text)',
    fontSize: 18,
    fontWeight: 800,
    letterSpacing: -0.2,
  },
  closeButton: {
    minHeight: 44,
    padding: '0 14px',
    borderRadius: 999,
    border: '1px solid color-mix(in srgb, var(--border) 74%, transparent)',
    background: 'rgba(255,255,255,0.04)',
    color: 'var(--text-muted)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 700,
  },
  sheetBody: {
    overflowY: 'auto',
    display: 'grid',
    gap: 12,
  },
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
    fontSize: 12,
    fontWeight: 700,
    textTransform: 'uppercase',
    letterSpacing: 0.8,
  },
  input: {
    minHeight: 52,
    padding: '0 16px',
    borderRadius: 18,
    border: '1px solid color-mix(in srgb, var(--border) 74%, transparent)',
    background: 'rgba(255,255,255,0.04)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 15,
    outline: 'none',
  },
  textarea: {
    width: '100%',
    resize: 'vertical',
    minHeight: 112,
    padding: '14px 16px',
    borderRadius: 18,
    border: '1px solid color-mix(in srgb, var(--border) 74%, transparent)',
    background: 'rgba(255,255,255,0.04)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 15,
    outline: 'none',
  },
  primaryButton: {
    minHeight: 52,
    borderRadius: 18,
    border: 'none',
    background: 'var(--accent)',
    color: '#fff',
    fontFamily: 'inherit',
    fontSize: 15,
    fontWeight: 800,
  },
  primaryButtonDisabled: {
    opacity: 0.45,
  },
  secondaryButton: {
    minHeight: 48,
    borderRadius: 18,
    border: '1px solid color-mix(in srgb, var(--border) 74%, transparent)',
    background: 'rgba(255,255,255,0.04)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 14,
    fontWeight: 700,
  },
  errorText: {
    color: '#ff8d86',
    fontSize: 13,
    fontWeight: 700,
    lineHeight: 1.4,
  },
  pickerHeader: {
    display: 'grid',
    gap: 4,
    paddingBottom: 4,
  },
  pickerTrackTitle: {
    color: 'var(--text)',
    fontSize: 16,
    fontWeight: 800,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  pickerTrackMeta: {
    color: 'var(--text-muted)',
    fontSize: 13,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  list: {
    display: 'grid',
    gap: 8,
  },
  listRow: {
    minHeight: 56,
    padding: '0 16px',
    borderRadius: 18,
    border: '1px solid color-mix(in srgb, var(--border) 74%, transparent)',
    background: 'rgba(255,255,255,0.04)',
    color: 'var(--text)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 14,
    textAlign: 'left',
    fontFamily: 'inherit',
  },
  listRowMain: {
    minWidth: 0,
    display: 'grid',
    gap: 2,
    flex: 1,
  },
  listRowTitle: {
    fontSize: 15,
    fontWeight: 800,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  listRowMeta: {
    fontSize: 12,
    color: 'var(--text-muted)',
  },
  listRowStatus: {
    fontSize: 12,
    fontWeight: 700,
    color: 'var(--accent)',
    flexShrink: 0,
  },
  emptyText: {
    color: 'var(--text-muted)',
    fontSize: 13,
    lineHeight: 1.5,
    padding: '10px 4px 2px',
  },
  actionsList: {
    display: 'grid',
    gap: 10,
  },
  actionButton: {
    minHeight: 52,
    padding: '0 16px',
    borderRadius: 18,
    border: '1px solid color-mix(in srgb, var(--border) 74%, transparent)',
    background: 'rgba(255,255,255,0.04)',
    color: 'var(--text)',
    textAlign: 'left',
    fontFamily: 'inherit',
    fontSize: 15,
    fontWeight: 700,
  },
};
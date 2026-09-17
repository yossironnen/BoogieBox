/**
 * Defines mobile Mobile Mixes View behavior for the BoogieBox React client.
 *
 * Playback-focused: lists previously rendered BoogieMix outputs and lets the
 * user play, rename, delete, or jump to the source playlist. Creating a new
 * mix stays in `MobileBoogieMixPanel` on the playlist detail screen — this
 * view never renders a job.
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../../api';
import type { EntityId } from '../../entityId';
import type { BoogieMixOutput, Track } from '../../types';
import { mixOutputToTrack } from '../../components/PlaylistsView';
import ArtImage from '../../components/ArtImage';
import { hybridMobileContentStyles } from '../../hybridPreview';
import MobileBottomSheet from '../components/MobileBottomSheet';
import MobileConfirmationSheet from '../components/MobileConfirmationSheet';

function fmtMixDuration(seconds: number | null | undefined): string {
  if (!seconds) return '--';
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${Math.max(1, minutes)}m`;
}

function parseCoverAlbumId(raw: string | null): EntityId | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) && parsed.length > 0 ? parsed[0] : null;
  } catch {
    return null;
  }
}

const PlayGlyph = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z" /></svg>
);
const KebabGlyph = () => (
  <svg width="17" height="17" viewBox="0 0 24 24" fill="currentColor">
    <circle cx="5" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="19" cy="12" r="1.6" />
  </svg>
);
const RenameGlyph = () => (
  <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 20h9" /><path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4Z" />
  </svg>
);
const OpenPlaylistGlyph = () => (
  <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
    <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><polyline points="15 3 21 3 21 9" /><line x1="10" y1="14" x2="21" y2="3" />
  </svg>
);
const TrashGlyph = () => (
  <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
    <polyline points="3 6 5 6 21 6" /><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" /><path d="M10 11v6" /><path d="M14 11v6" />
    <path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
  </svg>
);

function MobileRenameMixSheet({
  output,
  onClose,
  onSubmit,
}: {
  output: BoogieMixOutput;
  onClose: () => void;
  onSubmit: (name: string) => Promise<unknown>;
}) {
  const [name, setName] = useState(output.name);
  const [submitting, setSubmitting] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    const timer = window.setTimeout(() => inputRef.current?.focus(), 30);
    return () => window.clearTimeout(timer);
  }, []);

  const save = async () => {
    const trimmed = name.trim();
    if (!trimmed || submitting) return;
    setSubmitting(true);
    try {
      await onSubmit(trimmed);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <MobileBottomSheet title="Rename Mix" onClose={onClose}>
      <div style={styles.sheetStack}>
        <input
          ref={inputRef}
          type="text"
          value={name}
          onChange={(event) => setName(event.target.value)}
          style={hybridMobileContentStyles.field}
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
        />
        <button
          type="button"
          style={{ ...styles.saveButton, ...(!name.trim() || submitting ? hybridMobileContentStyles.disabled : null) }}
          disabled={!name.trim() || submitting}
          onClick={() => void save()}
        >
          {submitting ? 'Saving…' : 'Save'}
        </button>
      </div>
    </MobileBottomSheet>
  );
}

/** Mobile Mixes View is part of this module's public API. */
export default function MobileMixesView({
  onPlayTrack,
  onOpenPlaylist,
}: {
  onPlayTrack: (track: Track, allTracks?: Track[]) => void;
  onOpenPlaylist: (playlistId: EntityId) => void;
}) {
  const [outputs, setOutputs] = useState<BoogieMixOutput[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [actionsFor, setActionsFor] = useState<BoogieMixOutput | null>(null);
  const [renameTarget, setRenameTarget] = useState<BoogieMixOutput | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<BoogieMixOutput | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const next = await api.boogiemix?.listAllOutputs();
      setOutputs(next ?? []);
    } catch (err: any) {
      setError(err?.message || 'Could not load mixes.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const handlePlay = useCallback((output: BoogieMixOutput) => {
    const track = mixOutputToTrack(output, output.playlist_name || output.name);
    onPlayTrack(track, [track]);
  }, [onPlayTrack]);

  const handleRename = useCallback(async (output: BoogieMixOutput, name: string) => {
    if (name === output.name) return;
    setOutputs((prev) => prev.map((entry) => (entry.id === output.id ? { ...entry, name } : entry)));
    try {
      await api.boogiemix?.renameOutput(output.id, name);
    } catch {
      await load();
    }
  }, [load]);

  const handleDelete = useCallback(async (output: BoogieMixOutput) => {
    setOutputs((prev) => prev.filter((entry) => entry.id !== output.id));
    try {
      await api.boogiemix?.deleteOutput(output.id);
    } catch {
      await load();
    }
  }, [load]);

  return (
    <>
      {error ? (
        <div role="alert" style={styles.error}>
          {error}
          <button type="button" style={styles.retryButton} onClick={() => void load()}>Retry</button>
        </div>
      ) : null}
      {loading ? <div role="status" style={styles.loadingState}>Loading mixes...</div> : null}
      {!loading && !error && outputs.length === 0 ? (
        <div role="status" style={hybridMobileContentStyles.empty}>
          <div style={hybridMobileContentStyles.emptyTitle}>No mixes yet.</div>
          <p style={hybridMobileContentStyles.emptyBody}>
            Open a playlist and use BoogieMix to build one — it shows up here once it finishes rendering.
          </p>
        </div>
      ) : null}
      {!loading && outputs.length > 0 ? (
        <div style={hybridMobileContentStyles.list}>
          {outputs.map((output) => {
            const coverAlbumId = parseCoverAlbumId(output.cover_album_ids);
            return (
              <div key={output.id} style={styles.row}>
                <button
                  type="button"
                  style={styles.playButton}
                  aria-label={`Play ${output.name}`}
                  onClick={() => handlePlay(output)}
                >
                  <span style={hybridMobileContentStyles.listArtwork}>
                    {coverAlbumId ? (
                      <ArtImage src={api.albumArtUrl(coverAlbumId, 300)} alt="" imgStyle={hybridMobileContentStyles.listArtworkImage} />
                    ) : (
                      <span style={hybridMobileContentStyles.listArtworkFallback}><PlayGlyph /></span>
                    )}
                  </span>
                  <span style={hybridMobileContentStyles.listMeta}>
                    <span style={hybridMobileContentStyles.listTitle}>{output.name}</span>
                    <span style={hybridMobileContentStyles.listSubtitle}>
                      {output.playlist_name ? `From ${output.playlist_name}` : 'Source playlist deleted'}
                    </span>
                  </span>
                  <span style={styles.rowDuration}>{fmtMixDuration(output.duration_sec)}</span>
                </button>
                <button
                  type="button"
                  style={styles.kebabButton}
                  aria-label={`More actions for ${output.name}`}
                  onClick={() => setActionsFor(output)}
                >
                  <KebabGlyph />
                </button>
              </div>
            );
          })}
        </div>
      ) : null}

      {actionsFor ? (
        <MobileBottomSheet title={actionsFor.name} onClose={() => setActionsFor(null)}>
          <div style={styles.sheetStack}>
            <button
              type="button"
              style={styles.sheetAction}
              onClick={() => { setRenameTarget(actionsFor); setActionsFor(null); }}
            >
              <RenameGlyph /> Rename
            </button>
            {actionsFor.playlist_id != null ? (
              <button
                type="button"
                style={styles.sheetAction}
                onClick={() => { onOpenPlaylist(actionsFor.playlist_id as EntityId); setActionsFor(null); }}
              >
                <OpenPlaylistGlyph /> Open source playlist
              </button>
            ) : null}
            <button
              type="button"
              style={{ ...styles.sheetAction, color: 'var(--danger)' }}
              onClick={() => { setDeleteTarget(actionsFor); setActionsFor(null); }}
            >
              <TrashGlyph /> Delete mix
            </button>
          </div>
        </MobileBottomSheet>
      ) : null}

      {renameTarget ? (
        <MobileRenameMixSheet
          output={renameTarget}
          onClose={() => setRenameTarget(null)}
          onSubmit={async (name) => {
            await handleRename(renameTarget, name);
            setRenameTarget(null);
          }}
        />
      ) : null}

      <MobileConfirmationSheet
        open={deleteTarget !== null}
        title="Delete this mix?"
        description="This removes the rendered mix file. The source playlist and its tracks are not affected."
        itemLabel={deleteTarget?.name}
        confirmLabel="Delete mix"
        busyLabel="Deleting…"
        onClose={() => setDeleteTarget(null)}
        onConfirm={() => (deleteTarget ? handleDelete(deleteTarget) : undefined)}
      />
    </>
  );
}

const styles: Record<string, React.CSSProperties> = {
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
    fontSize: 13,
    fontWeight: 750,
  },
  loadingState: {
    ...hybridMobileContentStyles.feedback,
    marginBottom: 12,
  },
  row: {
    display: 'flex',
    alignItems: 'stretch',
    gap: 6,
  },
  playButton: {
    flex: 1,
    minWidth: 0,
    minHeight: 66,
    display: 'grid',
    gridTemplateColumns: '48px minmax(0, 1fr) auto',
    alignItems: 'center',
    gap: 10,
    boxSizing: 'border-box',
    padding: '8px 10px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface)',
    color: 'var(--text)',
    textAlign: 'left',
    fontFamily: 'inherit',
  },
  rowDuration: {
    color: 'var(--text-muted)',
    fontSize: 12,
    fontWeight: 650,
    fontVariantNumeric: 'tabular-nums',
    flexShrink: 0,
  },
  kebabButton: {
    width: 44,
    minWidth: 44,
    display: 'grid',
    placeItems: 'center',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface)',
    color: 'var(--text-muted)',
    padding: 0,
    fontFamily: 'inherit',
  },
  sheetStack: { display: 'grid', gap: 10 },
  sheetAction: {
    minHeight: 52,
    display: 'flex',
    alignItems: 'center',
    gap: 12,
    borderRadius: 12,
    border: '1px solid var(--divider-subtle)',
    background: 'var(--surface)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 14,
    fontWeight: 750,
    textAlign: 'left',
    padding: '0 14px',
  },
  saveButton: {
    minHeight: 52,
    borderRadius: 12,
    border: 'none',
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    fontFamily: 'inherit',
    fontSize: 14,
    fontWeight: 750,
  },
};

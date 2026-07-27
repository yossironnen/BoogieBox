/**
 * Defines the Playlists View React component and related UI helpers.
 */

import React, { useState, useEffect, useCallback, useRef } from 'react';
import { api } from '../api';
import type { Playlist, PlaylistTrack, Track, CrossfadeMode, BoogieMixDeepAnalysisStatus, BoogieMixJob, ClientEntityId, PlaylistDeepAnalysisProgress } from '../types';
import type { EntityId } from '../entityId';
import { parseServerDate } from '../utils';
import { phase2 } from '../uiPhase2';
import {
  hybridControlStyles,
  hybridMediaStyles,
  hybridPlaylistStyles,
} from '../hybridPreview';
import ArtImage from './ArtImage';

// ─── Helpers ──────────────────────────────────────────────────────────────────

export function fmtDur(s: number | null | undefined): string {
  if (!s) return '';
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

export function normalizePlaylistName(name: string): string {
  return name.trim().replace(/\s+/g, ' ').toLowerCase();
}

export function buildPlaylistCollageAlbumIds(tracks: PlaylistTrack[]): ClientEntityId[] {
  const ids = new Set<ClientEntityId>();
  for (const track of tracks) {
    if (!track.album_id || ids.has(track.album_id)) continue;
    ids.add(track.album_id);
    if (ids.size === 4) break;
  }
  return Array.from(ids);
}

export function createPlaylistFallbackTiles(count: number): number[] {
  return Array.from({ length: Math.max(0, 4 - count) }, (_, index) => index);
}

export function fmtTrackDur(s: number | null): string {
  if (!s) return '–';
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${String(sec).padStart(2, '0')}`;
}

/** Format Boogie Mix Fallback Message is part of this module's public API. */
export function formatBoogieMixFallbackMessage(
  job: Pick<BoogieMixJob, 'mix_quality' | 'used_deep_analysis' | 'deep_analysis_missing_reason'> | null,
  status: BoogieMixDeepAnalysisStatus | null,
): string | null {
  if (job?.mix_quality === 'high_quality' && !job.used_deep_analysis) {
    return job.deep_analysis_missing_reason || 'Deep analysis was unavailable. This mix used standard analysis.';
  }
  if (!job && status?.runtime && !status.runtime.enabled) {
    const missing = status.runtime.missingCapabilities.length
      ? status.runtime.missingCapabilities.join(', ')
      : 'Python, Torch, Demucs, or FFmpeg';
    return `High Quality needs deep analysis. Missing: ${missing}. Mixes can still be created with standard analysis.`;
  }
  return null;
}

// ─── Icons ────────────────────────────────────────────────────────────────────

const PlayIcon    = () => <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor"><polygon points="5,3 19,12 5,21"/></svg>;
const PlusIcon    = () => <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>;
const TrashIcon   = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/><path d="M9 6V4h6v2"/></svg>;
const EditIcon    = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>;
const GripIcon    = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="9" cy="7" r="1" fill="currentColor"/><circle cx="15" cy="7" r="1" fill="currentColor"/><circle cx="9" cy="12" r="1" fill="currentColor"/><circle cx="15" cy="12" r="1" fill="currentColor"/><circle cx="9" cy="17" r="1" fill="currentColor"/><circle cx="15" cy="17" r="1" fill="currentColor"/></svg>;
const SearchIcon  = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>;
const ListIcon    = () => <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/></svg>;
const XIcon       = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>;
const QueueIcon   = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>;
const CrossfadeIcon  = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M2 20V4l10 8L2 20z" opacity="0.6"/><path d="M12 20V4l10 8-10 8z"/></svg>;
const BookmarkIcon   = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>;
const MixIcon        = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M17 3h4v4"/><path d="M3 21l18-18"/><path d="M21 17v4h-4"/><path d="M3 3l6 6"/><path d="M15 15l6 6"/></svg>;

// ─── New / Edit Playlist Dialog ───────────────────────────────────────────────

function PlaylistDialog({
  initial, onSave, onCancel, error,
}: {
  initial?: { name: string; description: string };
  onSave: (name: string, description: string) => void;
  onCancel: () => void;
  error?: string;
}) {
  const [name, setName]        = useState(initial?.name ?? '');
  const [desc, setDesc]        = useState(initial?.description ?? '');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => { inputRef.current?.focus(); }, []);

  const submit = () => { if (name.trim()) onSave(name.trim(), desc.trim()); };

  return (
    <div style={D.overlay} onClick={onCancel}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="playlist-dialog-title"
        style={D.dialog}
        onClick={e => e.stopPropagation()}
      >
        <div id="playlist-dialog-title" style={D.dialogTitle}>{initial ? 'Rename Playlist' : 'New Playlist'}</div>
        <input
          ref={inputRef}
          style={D.input}
          placeholder="Playlist name"
          value={name}
          onChange={e => setName(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') submit(); if (e.key === 'Escape') onCancel(); }}
        />
        <input
          style={D.input}
          placeholder="Description (optional)"
          value={desc}
          onChange={e => setDesc(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') submit(); if (e.key === 'Escape') onCancel(); }}
        />
        {error && <div style={D.errorText}>{error}</div>}
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
          <button type="button" style={D.cancelBtn} onClick={onCancel}>Cancel</button>
          <button type="button" style={{ ...D.saveBtn, ...(!name.trim() ? hybridControlStyles.disabled : {}) }} onClick={submit} disabled={!name.trim()}>
            {initial ? 'Save' : 'Create'}
          </button>
        </div>
      </div>
    </div>
  );
}

function DeletePlaylistDialog({
  playlistName,
  onConfirm,
  onCancel,
  error,
}: {
  playlistName: string;
  onConfirm: () => void;
  onCancel: () => void;
  error?: string;
}) {
  return (
    <div style={D.overlay} onClick={onCancel}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="delete-playlist-dialog-title"
        style={D.dialog}
        onClick={e => e.stopPropagation()}
      >
        <div style={D.dialogTitle}>BoogieBox</div>
        <div id="delete-playlist-dialog-title" style={{ fontSize: 13, color: 'var(--text)', fontWeight: 700 }}>
          Delete Playlist
        </div>
        <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.4 }}>
          Delete playlist "{playlistName}"?
        </div>
        {error && <div style={D.errorText}>{error}</div>}
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
          <button type="button" style={D.cancelBtn} onClick={onCancel}>Cancel</button>
          <button
            type="button"
            style={D.dangerBtn}
            onClick={onConfirm}
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}

const D: Record<string, React.CSSProperties> = {
  overlay: { position: 'fixed', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 200, ...hybridMediaStyles.overlay },
  dialog: { padding: 24, width: 400, display: 'flex', flexDirection: 'column', gap: 12, ...hybridMediaStyles.dialog },
  dialogTitle: { fontSize: 17, fontWeight: 750, color: 'var(--text)', marginBottom: 4, letterSpacing: -0.3 },
  input: { ...hybridControlStyles.field },
  errorText: { color: 'var(--danger)', fontSize: 12, lineHeight: 1.3 },
  saveBtn: { ...hybridControlStyles.primaryButton },
  cancelBtn: { ...hybridControlStyles.secondaryButton },
  dangerBtn: { ...hybridControlStyles.dangerButton },
};

// ─── Add Tracks Search Panel ──────────────────────────────────────────────────

function AddTracksPanel({
  playlistId, existingTrackIds, onAdded, onClose,
}: {
  playlistId: EntityId;
  existingTrackIds: Set<ClientEntityId>;
  onAdded: () => void;
  onClose: () => void;
}) {
  const [query, setQuery]     = useState('');
  const [results, setResults] = useState<Track[]>([]);
  const [adding, setAdding]   = useState<Set<ClientEntityId>>(new Set());
  const [added, setAdded]     = useState<Set<ClientEntityId>>(new Set(existingTrackIds));
  const debounce = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => { inputRef.current?.focus(); }, []);

  useEffect(() => {
    clearTimeout(debounce.current);
    if (!query.trim()) { setResults([]); return; }
    debounce.current = setTimeout(async () => {
      const res = await api.search({ q: query, limit: 50, page: 1 });
      setResults(res.tracks);
    }, 300);
    return () => clearTimeout(debounce.current);
  }, [query]);

  const addTrack = async (track: Track) => {
    if (added.has(track.id)) return;
    setAdding(prev => new Set(prev).add(track.id));
    try {
      await api.playlists.addTrack(playlistId, track.id);
      setAdded(prev => new Set(prev).add(track.id));
      onAdded();
    } finally {
      setAdding(prev => { const s = new Set(prev); s.delete(track.id); return s; });
    }
  };

  return (
    <div style={AP.panel}>
      <div style={AP.header}>
        <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)' }}>Add Tracks</div>
        <button type="button" aria-label="Close add tracks" style={AP.closeBtn} onClick={onClose}><XIcon /></button>
      </div>
      <div style={AP.searchRow}>
        <SearchIcon />
        <input
          ref={inputRef}
          style={AP.searchInput}
          placeholder="Search for tracks to add…"
          value={query}
          onChange={e => setQuery(e.target.value)}
        />
        {query && <button type="button" aria-label="Clear track search" style={AP.clearBtn} onClick={() => setQuery('')}><XIcon /></button>}
      </div>
      <div style={AP.results}>
        {!query && (
          <div style={AP.hint}>Type to search your library</div>
        )}
        {results.map(track => {
          const isAdded   = added.has(track.id);
          const isAdding  = adding.has(track.id);
          return (
            <div key={track.id} style={AP.row}>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={AP.trackTitle}>{track.title || track.file_name}</div>
                <div style={AP.trackSub}>{[track.artist, track.album].filter(Boolean).join(' · ')}</div>
              </div>
              <div style={AP.dur}>{fmtTrackDur(track.duration)}</div>
              <button
                type="button"
                style={{ ...AP.addBtn, opacity: isAdded ? 0.4 : 1 }}
                onClick={() => addTrack(track)}
                disabled={isAdded || isAdding}
                title={isAdded ? 'Already in playlist' : 'Add to playlist'}
              >
                {isAdded ? '✓' : isAdding ? '…' : <PlusIcon />}
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

const AP: Record<string, React.CSSProperties> = {
  panel: { display: 'flex', flexDirection: 'column', width: 340, flexShrink: 0, overflow: 'hidden', ...hybridMediaStyles.sidePanel },
  header: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '16px 16px 10px', borderBottom: 'none', flexShrink: 0 },
  closeBtn: { ...hybridControlStyles.iconButton, width: 32, minWidth: 32, height: 32, background: 'transparent' },
  searchRow: { display: 'flex', alignItems: 'center', gap: 8, margin: '0 12px 8px', padding: '7px 10px', border: '1px solid var(--border)', borderRadius: 10, color: 'var(--text-muted)', flexShrink: 0, background: 'var(--surface-subtle)' },
  searchInput: { flex: 1, background: 'transparent', border: 'none', outline: 'none', color: 'var(--text)', fontSize: 12, fontFamily: 'inherit', padding: '2px 0' },
  clearBtn: { background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', display: 'flex', alignItems: 'center', padding: 0 },
  results: { flex: 1, overflowY: 'auto', paddingBottom: 0 },
  hint: { padding: '24px 14px', textAlign: 'center', color: 'var(--text-muted)', fontSize: 12 },
  row: { display: 'flex', alignItems: 'center', gap: 8, margin: '2px 8px', padding: '8px 10px', border: 'none', borderRadius: 10, transition: 'background 0.1s' },
  trackTitle: { fontSize: 12, fontWeight: 500, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  trackSub: { fontSize: 10, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', marginTop: 2 },
  dur: { fontSize: 10, color: 'var(--text-muted)', flexShrink: 0, fontVariantNumeric: 'tabular-nums' },
  addBtn: { ...hybridControlStyles.tonalButton, padding: 0, minHeight: 30, width: 30, height: 30, flexShrink: 0, fontSize: 12 },
};

// ─── Draggable Track Row ──────────────────────────────────────────────────────

function DraggableTrackRow({
  track, index, total,
  onPlay, onRemove, onDragStart, onDragEnter, onDragEnd,
  isDragOver, isHovered, onHoverChange,
}: {
  track: PlaylistTrack; index: number; total: number;
  onPlay: () => void; onRemove: () => void;
  onDragStart: (i: number) => void;
  onDragEnter: (i: number) => void;
  onDragEnd: () => void;
  isDragOver: boolean;
  isHovered: boolean;
  onHoverChange: (hovered: boolean) => void;
}) {
  return (
    <div
      draggable
      onDragStart={() => onDragStart(index)}
      onDragEnter={() => onDragEnter(index)}
      onDragEnd={onDragEnd}
      onDragOver={e => e.preventDefault()}
      onClick={onPlay}
      onMouseEnter={() => onHoverChange(true)}
      onMouseLeave={() => onHoverChange(false)}
      style={{
        ...T.row,
        backgroundColor: isDragOver
          ? 'color-mix(in srgb, var(--accent) 10%, transparent)'
          : isHovered
            ? 'color-mix(in srgb, var(--accent) 12%, transparent)'
            : 'transparent',
        borderTop: isDragOver ? '2px solid var(--accent)' : '2px solid transparent',
      }}
    >
      <div style={T.grip} title="Drag to reorder"><GripIcon /></div>
      <div style={T.num}>{index + 1}</div>
      <button type="button" aria-label={`Play ${track.title || track.file_name}`} style={T.playBtn} onClick={(e) => { e.stopPropagation(); onPlay(); }} title="Play"><PlayIcon /></button>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={T.title}>{track.title || track.file_name}</div>
        <div style={T.sub}>{[track.artist, track.album].filter(Boolean).join(' · ')}</div>
      </div>
      {track.has_deep_analysis && (
        <span style={{ fontSize: 10, color: 'var(--accent)', opacity: 0.55, flexShrink: 0 }} title="Sonic Fingerprint available — AI stem analysis complete">✦</span>
      )}
      <div style={T.dur}>{fmtTrackDur(track.duration)}</div>
      <button type="button" aria-label={`Remove ${track.title || track.file_name} from playlist`} style={T.removeBtn} onClick={(e) => { e.stopPropagation(); onRemove(); }} title="Remove from playlist"><TrashIcon /></button>
    </div>
  );
}

const T: Record<string, React.CSSProperties> = {
  row: { display: 'flex', alignItems: 'center', gap: 8, cursor: 'grab', userSelect: 'none', transition: 'background 0.1s', ...hybridMediaStyles.listRow },
  grip: { color: 'var(--text-muted)', flexShrink: 0, opacity: 0.35, display: 'flex', alignItems: 'center', cursor: 'grab' },
  num: { width: 22, textAlign: 'right', fontSize: 11, color: 'var(--text-muted)', flexShrink: 0, fontVariantNumeric: 'tabular-nums' },
  playBtn: { background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', padding: '2px 3px', borderRadius: 4, display: 'flex', alignItems: 'center', opacity: 0.55, flexShrink: 0 },
  title: { fontSize: 12, fontWeight: 500, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  sub: { fontSize: 10, color: 'var(--text-muted)', marginTop: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  dur: { fontSize: 11, color: 'var(--text-muted)', flexShrink: 0, fontVariantNumeric: 'tabular-nums', width: 38, textAlign: 'right' },
  removeBtn: { background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', padding: '2px 4px', borderRadius: 4, display: 'flex', alignItems: 'center', opacity: 0.4, flexShrink: 0 },
};

// ─── Playlist Detail Panel ────────────────────────────────────────────────────

function PlaylistDetail({
  playlist, onUpdate, onDelete,
  playTrack, addToQueue,
}: {
  playlist: Playlist;
  onUpdate: () => void;
  onDelete: () => void;
  playTrack: (track: Track, all?: Track[], source?: import('../types').QueueSource) => void;
  addToQueue: (track: Track) => void;
}) {
  const [tracks, setTracks]       = useState<PlaylistTrack[]>([]);
  const [loading, setLoading]     = useState(true);
  const [loadError, setLoadError] = useState('');
  const [showAdd, setShowAdd]     = useState(false);
  const [showEdit, setShowEdit]   = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [deleteError, setDeleteError] = useState('');
  const [editError, setEditError] = useState('');
  const [dragFrom, setDragFrom]   = useState<number | null>(null);
  const [dragOver, setDragOver]   = useState<number | null>(null);
  const [showCrossfade, setShowCrossfade] = useState(false);
  const [cfMode, setCfMode]       = useState<CrossfadeMode>('off');
  const [cfDuration, setCfDuration] = useState(2);
  const [cfHasOverride, setCfHasOverride] = useState(false);
  const [cfSaving, setCfSaving]   = useState(false);
  const [rememberProgress, setRememberProgress] = useState(!!(playlist.remember_progress));
  const [hoveredTrackId, setHoveredTrackId] = useState<ClientEntityId | null>(null);
  const [mixJobId, setMixJobId] = useState<ClientEntityId | null>(null);
  const [mixJob, setMixJob] = useState<BoogieMixJob | null>(null);
  const [mixOutputs, setMixOutputs] = useState<any[]>([]);
  const [mixError, setMixError] = useState('');
  const [mixStyle, setMixStyle] = useState<'chill_blend' | 'club_blend' | 'long_build' | 'safe_mix'>('club_blend');
  const [mixQuality, setMixQuality] = useState<'standard' | 'high_quality'>('standard');
  const [mixCrossfade, setMixCrossfade] = useState(16);
  const [deepStatus, setDeepStatus] = useState<BoogieMixDeepAnalysisStatus | null>(null);
  const [deepRunning, setDeepRunning] = useState(false);
  const [deepProgress, setDeepProgress] = useState<PlaylistDeepAnalysisProgress | null>(null);
  const [deepQueuedCount, setDeepQueuedCount] = useState(0);
  const [deepError, setDeepError] = useState('');

  const loadTracks = useCallback(async () => {
    setLoading(true);
    setLoadError('');
    try {
      const t = await api.playlists.tracks(playlist.id);
      setTracks(t);
    } catch (e: any) {
      setTracks([]);
      setLoadError(e?.message || 'Could not load playlist tracks');
    } finally {
      setLoading(false);
    }
  }, [playlist.id]);

  useEffect(() => { loadTracks(); }, [loadTracks]);
  useEffect(() => {
    if (!api.boogiemix?.deepAnalysisStatus) {
      setDeepStatus(null);
      return;
    }
    api.boogiemix.deepAnalysisStatus().then(setDeepStatus).catch(() => setDeepStatus(null));
  }, [playlist.id]);

  useEffect(() => {
    if (!api.boogiemix) return;
    api.boogiemix.listOutputs(playlist.id).then(setMixOutputs).catch(() => {});
  }, [playlist.id]);
  useEffect(() => {
    if (!mixJobId) return;
    let stopped = false;
    const timer = setInterval(async () => {
      try {
        if (!api.boogiemix) return;
        const next = await api.boogiemix.getJob(mixJobId);
        if (stopped) return;
        setMixJob(next);
        if (next.status === 'done' || next.status === 'failed' || next.status === 'canceled') {
          clearInterval(timer);
          api.boogiemix.listOutputs(playlist.id).then(setMixOutputs).catch(() => {});
        }
      } catch {}
    }, 1200);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [mixJobId, playlist.id]);

  // Load crossfade config for this playlist
  useEffect(() => {
    api.crossfade.config('playlist', playlist.id).then(cfg => {
      setCfMode(cfg.mode);
      setCfDuration(cfg.duration);
      setCfHasOverride(cfg.source === 'override');
    }).catch(() => {});
  }, [playlist.id]);

  const remove = async (trackId: ClientEntityId) => {
    await api.playlists.removeTrack(playlist.id, trackId);
    setTracks(prev => prev.filter(t => t.id !== trackId));
    onUpdate();
  };

  const handleDragStart = (i: number) => setDragFrom(i);
  const handleDragEnter = (i: number) => setDragOver(i);

  const handleDragEnd = async () => {
    if (dragFrom === null || dragOver === null || dragFrom === dragOver) {
      setDragFrom(null); setDragOver(null); return;
    }
    const reordered = [...tracks];
    const [moved] = reordered.splice(dragFrom, 1);
    reordered.splice(dragOver, 0, moved);
    setTracks(reordered);
    setDragFrom(null); setDragOver(null);
    await api.playlists.reorder(playlist.id, reordered.map(t => t.id));
    onUpdate();
  };

  const handleEdit = async (name: string, description: string) => {
    try {
      await api.playlists.update(playlist.id, name, description);
      setEditError('');
      setShowEdit(false);
      onUpdate();
    } catch (e: any) {
      setEditError(e?.message || 'Could not update playlist');
    }
  };

  const handleDelete = async () => {
    try {
      await api.playlists.remove(playlist.id);
      setDeleteError('');
      setShowDeleteConfirm(false);
      onDelete();
    } catch (e: any) {
      setDeleteError(e?.message || 'Could not delete playlist');
    }
  };

  const handleToggleRememberProgress = async () => {
    const next = !rememberProgress;
    setRememberProgress(next);
    await api.playlists.update(playlist.id, playlist.name, playlist.description ?? '', next ? 1 : 0).catch(() => {});
  };

  const playAll   = () => { if (tracks.length) playTrack(tracks[0], tracks, { type: 'playlist', id: playlist.id, rememberProgress }); };
  const queueAll  = () => tracks.forEach(t => addToQueue(t));
  const totalDur  = tracks.reduce((a, t) => a + (t.duration ?? 0), 0);
  const collageAlbumIds = buildPlaylistCollageAlbumIds(tracks);
  const headerCollageAlbumIds = collageAlbumIds.length
    ? collageAlbumIds
    : (playlist.art_album_ids ?? []).slice(0, 4);
  const collageFallbackTiles = createPlaylistFallbackTiles(headerCollageAlbumIds.length);
  const deepFallbackMessage = formatBoogieMixFallbackMessage(
    mixJob,
    mixQuality === 'high_quality' ? deepStatus : null,
  );
  const usedDeepAnalysis = Boolean(mixJob?.used_deep_analysis);
  const planSummary = mixJob?.plan_summary;
  const energyCurvePhases = planSummary?.energyCurvePhases ?? [];
  const startBoogieMix = async () => {
    try {
      if (!api.boogiemix) return;
      setMixError('');
      const created = await api.boogiemix.createJob(playlist.id, mixStyle, mixQuality, mixCrossfade);
      setMixJobId(created.jobId);
      setMixJob(await api.boogiemix.getJob(created.jobId));
    } catch (e: any) {
      setMixError(e?.message || 'Failed to start BoogieMix');
    }
  };
  const cancelBoogieMix = async () => {
    if (!mixJobId) return;
    try {
      if (!api.boogiemix) return;
      await api.boogiemix.cancelJob(mixJobId);
      setMixJob(await api.boogiemix.getJob(mixJobId));
    } catch (e: any) {
      setMixError(e?.message || 'Failed to cancel BoogieMix job');
    }
  };

  const runDeepAnalysis = async () => {
    if (!api.boogiemix) return;
    setDeepError('');
    setDeepRunning(true);
    setDeepProgress(null);
    try {
      const result = await api.boogiemix.queuePlaylistDeepAnalysis(playlist.id);
      setDeepQueuedCount(result.queued);
      const prog = await api.boogiemix.playlistDeepAnalysisProgress(playlist.id);
      setDeepProgress(prog);
    } catch (e: any) {
      setDeepError(e?.message || 'Failed to queue deep analysis');
      setDeepRunning(false);
    }
  };

  useEffect(() => {
    if (!deepRunning || !api.boogiemix) return;
    let stopped = false;
    const timer = setInterval(async () => {
      try {
        const prog = await api.boogiemix!.playlistDeepAnalysisProgress(playlist.id);
        if (stopped) return;
        setDeepProgress(prog);
        if (prog.pending === 0 && prog.running === 0) {
          clearInterval(timer);
          setDeepRunning(false);
        }
      } catch {}
    }, 2000);
    return () => { stopped = true; clearInterval(timer); };
  }, [deepRunning, playlist.id]);

  return (
    <div data-ui-region="playlist-detail" style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0, overflow: 'hidden' }}>
      {/* Header */}
      <div style={PD.header}>
        <div style={PD.collage} aria-label={`${playlist.name} artwork`}>
          {headerCollageAlbumIds.map((albumId) => (
            <div key={albumId} style={PD.collageTile}>
              <ArtImage src={api.albumArtUrl(albumId, 300)} alt="" imgStyle={PD.collageArt} />
            </div>
          ))}
          {collageFallbackTiles.map((tile) => (
            <div key={`playlist-fallback-${tile}`} style={{ ...PD.collageTile, ...PD.collageFallback }} />
          ))}
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={PD.kicker}>Curated Collection</div>
          <div style={PD.name}>{playlist.name}</div>
          {playlist.description && <div style={PD.desc}>{playlist.description}</div>}
          <div style={PD.meta}>
            {[
              tracks.length ? `${tracks.length} tracks` : 'Empty',
              totalDur ? fmtDur(totalDur) : '',
              (() => { const d = parseServerDate(playlist.updated_at); return d ? `Updated ${d.toLocaleDateString()}` : null; })(),
            ].filter(Boolean).join(' · ')}
          </div>
        </div>
        <div style={PD.actionGroup}>
          <button type="button" style={{ ...PD.btnPrimary, ...(!tracks.length ? hybridControlStyles.disabled : {}) }} onClick={playAll} disabled={!tracks.length}>
            <PlayIcon /> Play All
          </button>
          <button type="button" style={{ ...PD.btnSecondary, ...(!tracks.length ? hybridControlStyles.disabled : {}) }} onClick={queueAll} disabled={!tracks.length}>
            <QueueIcon /> Queue All
          </button>
          <button
            type="button"
            style={{
              ...PD.btnSecondary,
              ...(!api.boogiemix || tracks.length < 2 || !!(mixJob && ['pending', 'analyzing', 'planning', 'rendering'].includes(mixJob.status))
                ? hybridControlStyles.disabled
                : {}),
            }}
            onClick={startBoogieMix}
            disabled={!api.boogiemix || tracks.length < 2 || !!(mixJob && ['pending', 'analyzing', 'planning', 'rendering'].includes(mixJob.status))}
            title="BoogieMix is experimental"
          >
            <MixIcon /> BoogieMix (Experimental)
          </button>
          <button
            type="button"
            style={{
              ...PD.btnSecondary,
              ...(!api.boogiemix || tracks.length === 0 || deepRunning ? hybridControlStyles.disabled : {}),
            }}
            onClick={runDeepAnalysis}
            disabled={!api.boogiemix || tracks.length === 0 || deepRunning}
            title="Run Demucs deep analysis on all tracks in this playlist. Replaces synthetic placeholder data with real AI stem analysis."
          >
            ⚡ Deep Analysis
          </button>
          <select
            value={mixStyle}
            onChange={(e) => setMixStyle(e.target.value as any)}
            style={{ ...PD.select, minWidth: 120 }}
            title="BoogieMix style"
          >
            <option value="chill_blend">Chill blend</option>
            <option value="club_blend">Club blend</option>
            <option value="long_build">Long build</option>
            <option value="safe_mix">Safe mix</option>
          </select>
          <select
            value={mixQuality}
            onChange={(e) => setMixQuality(e.target.value as any)}
            style={{ ...PD.select, minWidth: 164 }}
            title="BoogieMix quality"
          >
            <option value="standard">Standard</option>
            <option value="high_quality">High Quality (Deep Analysis)</option>
          </select>
          <select
            value={mixCrossfade}
            onChange={(e) => setMixCrossfade(Number(e.target.value))}
            style={{ ...PD.select, minWidth: 116 }}
            title="Transition length"
          >
            <option value={8}>8s blend</option>
            <option value={12}>12s blend</option>
            <option value={16}>16s blend</option>
            <option value={24}>24s blend</option>
            <option value={32}>32s blend</option>
            <option value={45}>45s blend</option>
          </select>
          <button type="button" style={showAdd ? PD.btnTonal : PD.btnSecondary} onClick={() => setShowAdd(s => !s)}>
            <PlusIcon /> Add Tracks
          </button>
          <button
            type="button"
            aria-label={rememberProgress ? 'Disable remembered track position' : 'Remember track position'}
            style={{ ...PD.iconBtn, ...(rememberProgress ? { color: 'var(--accent)', borderColor: 'var(--accent)' } : {}) }}
            onClick={handleToggleRememberProgress}
            title={rememberProgress ? 'Remember track position: On' : 'Remember track position: Off'}
          >
            <BookmarkIcon />
          </button>
          <button
            type="button"
            aria-label="Crossfade settings"
            style={{ ...PD.iconBtn, ...(showCrossfade ? { color: 'var(--accent)', borderColor: 'var(--accent)' } : {}) }}
            onClick={() => setShowCrossfade(s => !s)}
            title="Crossfade settings"
          >
            <CrossfadeIcon />
          </button>
          <button type="button" aria-label="Rename playlist" style={PD.iconBtn} onClick={() => { setEditError(''); setShowEdit(true); }} title="Rename"><EditIcon /></button>
          <button
            type="button"
            aria-label="Delete playlist"
            style={PD.iconDanger}
            onClick={() => { setDeleteError(''); setShowDeleteConfirm(true); }}
            title="Delete playlist"
          >
            <TrashIcon />
          </button>
        </div>
      </div>

      {/* Crossfade override panel */}
      {showCrossfade && (
        <div style={PD.crossfadePanel}>
          <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 0.8, marginBottom: 8 }}>
            Crossfade override
          </div>
          {/* Mode pills */}
          <div style={{ ...PD.segmentedGroup, marginBottom: 8 }}>
            {([
              { value: 'off' as const, label: 'Off' },
              { value: 'zerogap' as const, label: 'Zero-gap' },
              { value: 'crossfade' as const, label: 'Crossfade' },
            ]).map(opt => (
              <button
                key={opt.value}
                onClick={async () => {
                  setCfMode(opt.value);
                  setCfSaving(true);
                  await api.crossfade.upsertOverride({ entity_type: 'playlist', entity_id: playlist.id, mode: opt.value, duration: cfDuration }).catch(() => {});
                  setCfHasOverride(true);
                  setCfSaving(false);
                }}
                style={{
                  ...PD.segment,
                  ...(cfMode === opt.value ? PD.segmentActive : {}),
                }}
              >
                {opt.label}
              </button>
            ))}
          </div>
          {/* Duration slider */}
          {cfMode === 'crossfade' && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
              <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>1s</span>
              <input
                type="range" min={1} max={10} step={1} value={cfDuration}
                onChange={async e => {
                  const v = Number(e.target.value);
                  setCfDuration(v);
                  setCfSaving(true);
                  await api.crossfade.upsertOverride({ entity_type: 'playlist', entity_id: playlist.id, mode: cfMode, duration: v }).catch(() => {});
                  setCfHasOverride(true);
                  setCfSaving(false);
                }}
                style={{ flex: 1, maxWidth: 200, accentColor: 'var(--accent)' }}
              />
              <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>10s</span>
              <span style={{ fontSize: 11, fontWeight: 600, color: 'var(--text)' }}>{cfDuration}s</span>
            </div>
          )}
          {/* Reset + status */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            {cfHasOverride && (
              <button
                onClick={async () => {
                  setCfSaving(true);
                  await api.crossfade.removeOverride('playlist', playlist.id).catch(() => {});
                  const cfg = await api.crossfade.config('playlist', playlist.id).catch(() => ({ mode: 'off' as const, duration: 2, source: 'global' as const }));
                  setCfMode(cfg.mode);
                  setCfDuration(cfg.duration);
                  setCfHasOverride(cfg.source === 'override');
                  setCfSaving(false);
                }}
                style={{
                  ...PD.btnSecondary,
                  minHeight: 30,
                  padding: '5px 9px',
                  fontSize: 10,
                }}
              >
                Reset to default
              </button>
            )}
            {cfSaving && <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>Saving…</span>}
            {!cfHasOverride && !cfSaving && <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>Using global default</span>}
          </div>
        </div>
      )}

      {/* Body: tracks + optional add panel */}
      <div style={{ flex: 1, display: 'flex', minHeight: 0, overflow: 'hidden' }}>
        {/* Track list */}
        <div style={{ flex: 1, overflowY: 'auto', paddingBottom: 0 }}>
          {(deepRunning || deepProgress || deepError || mixJob || mixError || mixOutputs.length > 0 || deepFallbackMessage) && (
            <div style={PD.statusPanel}>
              <div style={{ fontSize: 11, color: 'var(--warning)', fontWeight: 650 }}>
                BoogieMix is experimental and may produce inconsistent results.
              </div>
              {(deepRunning || deepProgress || deepError) && (
                <DeepAnalysisProgressPanel
                  progress={deepProgress}
                  running={deepRunning}
                  queuedCount={deepQueuedCount}
                  error={deepError}
                  onDismiss={() => { setDeepProgress(null); setDeepError(''); setDeepRunning(false); }}
                />
              )}
              {mixJob && (
                <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>
                  BoogieMix: {mixJob.status} · {mixJob.progress_percent ?? 0}% {mixJob.current_step ? `· ${mixJob.current_step}` : ''}
                </div>
              )}
              {mixJob && (
                <div style={{ marginTop: 2, fontSize: 11, color: 'var(--text-muted)' }}>
                  Quality: {mixJob.mix_quality === 'high_quality' ? 'High Quality (Deep Analysis)' : 'Standard'}
                  {usedDeepAnalysis ? ' · Demucs-enhanced planning used' : ' · Standard analysis path'}
                </div>
              )}
              {deepFallbackMessage && (
                <div style={{ marginTop: 2, fontSize: 11, color: 'var(--warning)' }}>
                  {deepFallbackMessage}
                </div>
              )}
              {deepStatus && (
                <div style={{ marginTop: 2, fontSize: 11, color: 'var(--text-muted)' }}>
                  Deep analysis runtime: {deepStatus.runtime?.summary ?? (deepStatus.runtime?.enabled ? 'Ready' : 'Unavailable')} · Queue {deepStatus.queue?.pending ?? 0} pending
                  {mixJob?.deep_analysis_total_count ? ` · Ready ${mixJob.deep_analysis_ready_count ?? 0}/${mixJob.deep_analysis_total_count}` : ''}
                </div>
              )}
              {mixJob?.mix_strategy && (
                <div style={{ marginTop: 4, fontSize: 11, color: 'var(--text-muted)' }}>
                  AI Mix Strategy: {mixJob.mix_strategy}
                </div>
              )}
              {energyCurvePhases.length > 0 && (
                <div style={{ marginTop: 4, fontSize: 11, color: 'var(--text-muted)' }}>
                  {energyCurvePhases.join(' → ')}
                </div>
              )}
              {planSummary?.anthemTrackId && (
                <div style={{ marginTop: 2, fontSize: 11, color: 'var(--text-muted)' }}>
                  Anthem Track ID: {planSummary.anthemTrackId}
                </div>
              )}
              {mixJob && ['pending', 'analyzing', 'planning'].includes(mixJob.status) && (
                <button style={{ ...PD.btnSecondary, marginTop: 6, padding: '4px 8px', fontSize: 11 }} onClick={cancelBoogieMix}>
                  Cancel
                </button>
              )}
              {mixJob?.last_message && <div style={{ marginTop: 4, fontSize: 11, color: 'var(--text-muted)' }}>{mixJob.last_message}</div>}
              {mixError && <div style={{ marginTop: 4, fontSize: 11, color: 'var(--danger)' }}>{mixError}</div>}
              {mixOutputs[0] && (
                <div style={{ marginTop: 6, fontSize: 11, color: 'var(--text)' }}>
                  Latest mix: {mixOutputs[0].file_name}
                  <a href={api.boogiemix ? api.boogiemix.outputDownloadUrl(mixOutputs[0].id) : '#'} style={{ marginLeft: 8, color: 'var(--accent)', textDecoration: 'none' }}>
                    Download
                  </a>
                </div>
              )}
            </div>
          )}
          {loading && <div style={PD.empty}>Loading…</div>}
          {!loading && loadError && (
            <div style={PD.empty}>
              <ListIcon />
              <div style={{ marginTop: 12, fontSize: 14, fontWeight: 500, color: 'var(--text)' }}>Could not load tracks</div>
              <div style={{ marginTop: 6, fontSize: 12, color: 'var(--text-muted)' }}>{loadError}</div>
            </div>
          )}
          {!loading && !loadError && tracks.length === 0 && (
            <div style={PD.empty}>
              <ListIcon />
              <div style={{ marginTop: 12, fontSize: 14, fontWeight: 500, color: 'var(--text)' }}>It’s Oh So Quiet</div>
              <div style={{ marginTop: 6, fontSize: 12, color: 'var(--text-muted)' }}>Click "Add Tracks" to search and add music</div>
            </div>
          )}
          {tracks.map((track, i) => (
            <DraggableTrackRow
              key={track.playlist_track_id ?? `track-${i}`}
              track={track}
              index={i}
              total={tracks.length}
              isDragOver={dragOver === i && dragFrom !== i}
              onPlay={() => playTrack(track, tracks, { type: 'playlist', id: playlist.id, rememberProgress })}
              onRemove={() => remove(track.id)}
              onDragStart={handleDragStart}
              onDragEnter={handleDragEnter}
              onDragEnd={handleDragEnd}
              isHovered={hoveredTrackId === track.id}
              onHoverChange={(hovered) => setHoveredTrackId(hovered ? track.id : null)}
            />
          ))}
        </div>

        {/* Add tracks slide-in panel */}
        {showAdd && (
          <AddTracksPanel
            playlistId={playlist.id}
            existingTrackIds={new Set(tracks.map(t => t.id))}
            onAdded={() => { loadTracks(); onUpdate(); }}
            onClose={() => setShowAdd(false)}
          />
        )}
      </div>

      {showEdit && (
        <PlaylistDialog
          initial={{ name: playlist.name, description: playlist.description ?? '' }}
          onSave={handleEdit}
          onCancel={() => { setEditError(''); setShowEdit(false); }}
          error={editError}
        />
      )}

      {showDeleteConfirm && (
        <DeletePlaylistDialog
          playlistName={playlist.name}
          onConfirm={handleDelete}
          onCancel={() => { setDeleteError(''); setShowDeleteConfirm(false); }}
          error={deleteError}
        />
      )}
    </div>
  );
}

const PD: Record<string, React.CSSProperties> = {
  header: { ...phase2.desktopHero, display: 'flex', alignItems: 'flex-start', flexShrink: 0, flexWrap: 'wrap', ...hybridPlaylistStyles.detailHeader },
  collage: {
    width: 136,
    aspectRatio: '1 / 1',
    display: 'grid',
    gridTemplateColumns: 'repeat(2, 1fr)',
    gap: 4,
    padding: 4,
    flexShrink: 0,
    ...hybridMediaStyles.artworkFrame,
  },
  collageTile: { minWidth: 0, minHeight: 0, borderRadius: 10, overflow: 'hidden', background: 'var(--surface-subtle)' },
  collageArt: { width: '100%', height: '100%', objectFit: 'cover', display: 'block' },
  collageFallback: { background: 'linear-gradient(135deg, color-mix(in srgb, var(--accent) 20%, var(--surface)), color-mix(in srgb, var(--surface) 88%, var(--bg)))' },
  kicker: phase2.eyebrow,
  name: { color: 'var(--text)', marginTop: 6, marginBottom: 6, lineHeight: 1.02, ...hybridPlaylistStyles.detailName },
  desc: { fontSize: 13, color: 'color-mix(in srgb, var(--text) 86%, var(--text-muted))', marginBottom: 8, maxWidth: 620, lineHeight: 1.55 },
  meta: { fontSize: 12, color: 'var(--text-muted)', fontWeight: 600 },
  empty: { ...phase2.desktopMediaRow, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', margin: 24, padding: '72px 20px', color: 'var(--text-muted)', fontSize: 13, ...hybridMediaStyles.emptyState },
  actionGroup: { ...hybridPlaylistStyles.actionGroup },
  btnPrimary: { ...hybridControlStyles.primaryButton },
  btnSecondary: { ...hybridControlStyles.secondaryButton },
  btnTonal: { ...hybridControlStyles.tonalButton },
  iconBtn: { ...hybridControlStyles.iconButton },
  iconDanger: { ...hybridControlStyles.iconButton, background: 'color-mix(in srgb, var(--danger) 14%, transparent)', color: 'var(--danger)' },
  select: { ...hybridControlStyles.select },
  segmentedGroup: { ...hybridControlStyles.segmentedGroup },
  segment: { ...hybridControlStyles.segment },
  segmentActive: { ...hybridControlStyles.segmentActive },
  crossfadePanel: { ...hybridPlaylistStyles.crossfadePanel },
  statusPanel: { ...hybridPlaylistStyles.statusPanel },
};

// ─── Deep Analysis Progress Panel ────────────────────────────────────────────

function DeepAnalysisProgressPanel({
  progress,
  running,
  queuedCount,
  error,
  onDismiss,
}: {
  progress: PlaylistDeepAnalysisProgress | null;
  running: boolean;
  queuedCount: number;
  error: string;
  onDismiss: () => void;
}) {
  const total = progress?.total ?? 0;
  const done = (progress?.done ?? 0) + (progress?.skipped ?? 0);
  const active = progress?.running ?? 0;
  const pending = progress?.pending ?? 0;
  const analyzedReal = progress?.analyzedReal ?? 0;
  const analyzedCached = progress?.analyzedCached ?? analyzedReal;
  const analyzedFallback = progress?.analyzedFallback ?? Math.max(0, analyzedCached - analyzedReal);
  const pct = total > 0 ? Math.round((done / total) * 100) : 0;
  const finished = !running && total > 0;

  return (
    <div style={{ marginTop: 8, padding: '10px 12px', borderRadius: 10, backgroundColor: 'var(--surface)', border: '1px solid var(--divider-subtle)' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6 }}>
        <span style={{ fontSize: 11, fontWeight: 700, color: 'var(--text)' }}>
          {running ? '⚡ Deep Analysis Running' : finished ? '✓ Deep Analysis Complete' : '⚡ Deep Analysis'}
        </span>
        <button
          onClick={onDismiss}
          style={{ marginLeft: 'auto', background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-muted)', fontSize: 13, lineHeight: 1, padding: '0 2px' }}
          title="Dismiss"
        >
          ×
        </button>
      </div>
      {error && <div style={{ fontSize: 11, color: 'var(--danger)', marginBottom: 4 }}>{error}</div>}
      {progress && (
        <>
          <div style={{ width: '100%', height: 6, borderRadius: 3, backgroundColor: 'var(--border)', overflow: 'hidden', marginBottom: 6 }}>
            <div style={{ height: '100%', width: `${pct}%`, borderRadius: 3, backgroundColor: finished ? 'var(--success)' : 'var(--accent)', transition: 'width 0.4s ease' }} />
          </div>
          <div style={{ fontSize: 11, color: 'var(--text-muted)', display: 'flex', gap: 12, flexWrap: 'wrap' }}>
            <span>{done}/{total} processed</span>
            {active > 0 && <span style={{ color: 'var(--accent)' }}>{active} running</span>}
            {pending > 0 && <span>{pending} queued</span>}
            <span>{analyzedCached} cached</span>
            <span style={{ color: 'var(--success)' }}>{analyzedReal} with real analysis</span>
            {analyzedFallback > 0 && <span style={{ color: 'var(--warning)' }}>{analyzedFallback} fallback</span>}
            {progress.failed > 0 && <span style={{ color: 'var(--danger)' }}>{progress.failed} failed</span>}
          </div>
          {running && active > 0 && (
            <div style={{ fontSize: 10, color: 'var(--text-muted)', marginTop: 4 }}>
              Processing with Demucs CPU analysis — this may take several minutes per track.
            </div>
          )}
          {finished && (
            <div style={{ fontSize: 11, color: 'var(--success)', marginTop: 4 }}>
              {analyzedCached} track{analyzedCached !== 1 ? 's' : ''} have saved deep-analysis data.
              {analyzedFallback > 0 ? ` ${analyzedFallback} are fallback rows; real Demucs stem analysis did not complete for those tracks.` : ''}
              {progress.failed > 0 ? ` ${progress.failed} failed.` : ''}
            </div>
          )}
        </>
      )}
      {!progress && running && (
        <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>
          Queued {queuedCount} track{queuedCount !== 1 ? 's' : ''} for analysis…
        </div>
      )}
    </div>
  );
}

// ─── Sidebar: Playlist List ───────────────────────────────────────────────────

function PlaylistSidebar({
  playlists, selectedId, onSelect, onCreate,
}: {
  playlists: Playlist[];
  selectedId: EntityId | null;
  onSelect: (p: Playlist) => void;
  onCreate: () => void;
}) {
  return (
    <div data-ui-region="playlist-sidebar" style={SB.sidebar}>
      <div style={SB.header}>
        <span style={{ fontWeight: 600, fontSize: 13, color: 'var(--text)' }}>Playlists</span>
        <button type="button" aria-label="New playlist" style={SB.newBtn} onClick={onCreate} title="New playlist">
          <PlusIcon />
        </button>
      </div>
      <div style={SB.list}>
        {playlists.length === 0 && (
          <div style={SB.empty}>It's lonely here. Click + to create a new playlist.</div>
        )}
        {playlists.map(pl => (
          <button
            type="button"
            key={pl.id}
            style={{ ...SB.item, ...(selectedId === pl.id ? SB.itemActive : {}) }}
            onClick={() => onSelect(pl)}
            aria-current={selectedId === pl.id ? 'true' : undefined}
          >
            <div style={SB.itemName}>{pl.name}</div>
            <div style={SB.itemMeta}>
              {pl.track_count} track{pl.track_count !== 1 ? 's' : ''}
              {pl.total_duration ? ` · ${fmtDur(pl.total_duration)}` : ''}
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}

const SB: Record<string, React.CSSProperties> = {
  sidebar: { display: 'flex', flexDirection: 'column', flexShrink: 0, ...hybridPlaylistStyles.sidebar },
  header: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexShrink: 0, ...hybridPlaylistStyles.sidebarHeader },
  newBtn: { ...hybridControlStyles.iconButton, width: 34, minWidth: 34, height: 34, background: 'var(--accent)', color: 'var(--on-accent)' },
  list: { flex: 1, overflowY: 'auto', paddingBottom: 0 },
  empty: { padding: '28px 16px', fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.7, textAlign: 'center' },
  item: { display: 'block', textAlign: 'left', cursor: 'pointer', fontFamily: 'inherit', transition: 'background 0.12s, color 0.12s', ...hybridPlaylistStyles.sidebarItem },
  itemActive: { ...hybridPlaylistStyles.sidebarItemActive },
  itemName: { fontSize: 14, fontWeight: 700, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', marginBottom: 4 },
  itemMeta: { fontSize: 11, color: 'var(--text-muted)' },
};

// ─── Top-level PlaylistsView ──────────────────────────────────────────────────

interface Props {
  playTrack: (track: Track, all?: Track[], source?: import('../types').QueueSource) => void;
  addToQueue: (track: Track) => void;
  initialPlaylistId?: EntityId | null;
}

/** Playlists View is part of this module's public API. */
export default function PlaylistsView({ playTrack, addToQueue, initialPlaylistId }: Props) {
  const [playlists, setPlaylists]   = useState<Playlist[]>([]);
  const [selected, setSelected]     = useState<Playlist | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [createError, setCreateError] = useState('');

  const loadPlaylists = useCallback(async () => {
    const list = await api.playlists.list();
    setPlaylists(list);
    // Re-sync the selected playlist object (name/count may have changed)
    if (selected) {
      const updated = list.find((p: Playlist) => p.id === selected.id);
      if (updated) setSelected(updated);
    }
  }, [selected]);

  useEffect(() => { loadPlaylists(); }, [loadPlaylists]);

  // If a playlist id was passed in (e.g. from "Add to playlist" elsewhere), auto-select it
  useEffect(() => {
    if (initialPlaylistId && playlists.length) {
      const pl = playlists.find(p => p.id === initialPlaylistId);
      if (pl) setSelected(pl);
    }
  }, [initialPlaylistId, playlists]);

  const handleCreate = async (name: string, description: string) => {
    const normalizedName = normalizePlaylistName(name);
    if (playlists.some((playlist) => normalizePlaylistName(playlist.name) === normalizedName)) {
      setCreateError('A playlist with this name already exists');
      return;
    }
    try {
      const pl = await api.playlists.create(name, description);
      setCreateError('');
      setShowCreate(false);
      await loadPlaylists();
      setSelected(pl);
    } catch (e: any) {
      setCreateError(e?.message || 'Could not create playlist');
    }
  };

  const handleDeleted = async () => {
    setSelected(null);
    await loadPlaylists();
  };

  return (
    <div
      data-ui-design="hybrid"
      style={{ flex: 1, display: 'flex', minHeight: 0, overflow: 'hidden', ...hybridPlaylistStyles.root }}
    >
      <PlaylistSidebar
        playlists={playlists}
        selectedId={selected?.id ?? null}
        onSelect={pl => setSelected(pl)}
        onCreate={() => { setCreateError(''); setShowCreate(true); }}
      />

      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0, overflow: 'hidden' }}>
        {!selected && (
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', flex: 1, gap: 16, color: 'var(--text-muted)' }}>
            <ListIcon />
            <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text)' }}>Select a playlist</div>
            <div style={{ fontSize: 12 }}>Or create a new one with the + button</div>
            <button type="button" style={PD.btnPrimary} onClick={() => { setCreateError(''); setShowCreate(true); }}>
              <PlusIcon /> New Playlist
            </button>
          </div>
        )}
        {selected && (
          <PlaylistDetail
            key={selected.id}
            playlist={selected}
            onUpdate={loadPlaylists}
            onDelete={handleDeleted}
            playTrack={playTrack}
            addToQueue={addToQueue}
          />
        )}
      </div>

      {showCreate && (
        <PlaylistDialog
          onSave={handleCreate}
          onCancel={() => { setCreateError(''); setShowCreate(false); }}
          error={createError}
        />
      )}
    </div>
  );
}

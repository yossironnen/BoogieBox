/**
 * Defines the Playlists View React component and related UI helpers.
 */

import React, { useState, useEffect, useCallback, useRef } from 'react';
import { createPortal } from 'react-dom';
import { api } from '../api';
import type { Playlist, PlaylistSnapshot, PlaylistTrack, Track, CrossfadeMode, BoogieMixDeepAnalysisStatus, BoogieMixJob, BoogieMixOutput, ClientEntityId, PlaylistDeepAnalysisProgress } from '../types';
import type { EntityId } from '../entityId';
import { parseServerDate } from '../utils';
import { phase2 } from '../uiPhase2';
import {
  hybridControlStyles,
  hybridMediaStyles,
  hybridPlaylistStyles,
} from '../hybridPreview';
import ArtImage from './ArtImage';
import CommandBar from './CommandBar';
import { ImportIcons, PlaylistImportDialog, PlaylistImportToast, type ImportedPlaylist } from './PlaylistImport';
import { PLAYLIST_FILE_ACCEPT } from '../playlistImport';
import SmartPlaylistEditor, { SmartIcon } from './SmartPlaylistEditor';

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

const PlayIcon    = ({ size = 11 }: { size?: number }) => <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor"><polygon points="5,3 19,12 5,21"/></svg>;
const PlusIcon    = ({ size = 13 }: { size?: number }) => <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>;
const TrashIcon   = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/><path d="M9 6V4h6v2"/></svg>;
const EditIcon    = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>;
const GripIcon    = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="9" cy="7" r="1" fill="currentColor"/><circle cx="15" cy="7" r="1" fill="currentColor"/><circle cx="9" cy="12" r="1" fill="currentColor"/><circle cx="15" cy="12" r="1" fill="currentColor"/><circle cx="9" cy="17" r="1" fill="currentColor"/><circle cx="15" cy="17" r="1" fill="currentColor"/></svg>;
const SearchIcon  = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>;
const ListIcon    = () => <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/></svg>;
const XIcon       = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>;
const ChevronLeftIcon = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="15 18 9 12 15 6"/></svg>;
const QueueIcon   = ({ size = 12 }: { size?: number }) => <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>;
// Same crossing-arrows glyph as the album/artist Shuffle Play icon and the
// player's own shuffle button, for consistency.
const ShuffleIcon = () => <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="16 3 21 3 21 8"/><line x1="4" y1="20" x2="21" y2="3"/><polyline points="21 16 21 21 16 21"/><line x1="15" y1="15" x2="21" y2="21"/><line x1="4" y1="4" x2="9" y2="9"/></svg>;
// Mixer sliders — was a crossing-arrows glyph indistinguishable from Shuffle.
const MixIcon        = ({ size = 12 }: { size?: number }) => <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="4" width="18" height="4" rx="1"/><rect x="3" y="10" width="12" height="4" rx="1"/><rect x="3" y="16" width="15" height="4" rx="1"/></svg>;
const NoteIcon        = () => <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true"><path d="M9 18V5l10-1v12"/><circle cx="7" cy="18" r="2.5"/><circle cx="17" cy="16" r="2.5"/></svg>;
const SpinnerIcon    = () => <svg className="sidebar-scan-spin" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true"><path d="M20 7v5h-5"/><path d="M18.2 17.2A8 8 0 1 1 20 12"/></svg>;
const CheckIcon      = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true"><polyline points="20 6 9 17 4 12"/></svg>;
const AlertIcon      = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>;
// Playlist detail stat icons (track count reuses QueueIcon's note glyph).
const ClockIcon      = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>;
const SparkleIcon    = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M12 3l1.9 5.6L19.5 10l-5.6 1.9L12 17.5l-1.9-5.6L4.5 10l5.6-1.4z"/></svg>;
const CopyIcon       = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><rect x="8" y="8" width="13" height="13" rx="2"/><path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3"/></svg>;
const CalendarIcon   = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><rect x="3" y="5" width="18" height="16" rx="2"/><path d="M16 3v4M8 3v4M3 11h18"/></svg>;

/** Job statuses where a BoogieMix render is still actively working. */
const MIX_ACTIVE_STATUSES: BoogieMixJob['status'][] = ['pending', 'analyzing', 'planning', 'rendering'];
const MIX_STEP_LABEL: Record<string, string> = {
  pending: 'Queued', analyzing: 'Analyzing', planning: 'Planning', rendering: 'Rendering',
};


/** Shared artwork for the playlist header and sidebar; no per-row track fetches. */
export function PlaylistArtwork({ albumIds, compact = false, responsive = false, thumb = false }: { albumIds: ClientEntityId[]; compact?: boolean; responsive?: boolean; thumb?: boolean }) {
  const ids = albumIds.slice(0, 4);
  return (
    <div style={{
      ...PD.collage,
      ...(compact ? { width: 70, padding: 3, gap: 3, borderRadius: 8 } : {}),
      ...(thumb ? { width: 38, padding: 2, gap: 2, borderRadius: 7 } : {}),
      ...(responsive ? { width: '100%', aspectRatio: '1 / 1' } : {}),
    }}>
      {ids.map(id => (
        <div key={id} style={{ ...PD.collageTile, ...(compact ? { borderRadius: 4 } : {}), ...(thumb ? { borderRadius: 2 } : {}) }}>
          <ArtImage src={api.albumArtUrl(id, 300)} alt="" imgStyle={PD.collageArt} />
        </div>
      ))}
      {createPlaylistFallbackTiles(ids.length).map(tile => (
        <div key={`fallback-${tile}`} style={{ ...PD.collageTile, ...PD.collageFallback }} />
      ))}
    </div>
  );
}

function PlaylistKebab({ onClick }: { onClick: () => void }) {
  return (
    <button type="button" aria-label="More actions" aria-haspopup="dialog"
      style={{ ...PD.iconBtn, background: 'transparent', border: 'none', flexShrink: 0 }}
      onClick={onClick}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
        <circle cx="12" cy="5" r="2" /><circle cx="12" cy="12" r="2" /><circle cx="12" cy="19" r="2" />
      </svg>
    </button>
  );
}

/** Native modal supplies focus containment and prevents interaction behind a popup. */
function PlaylistPopup({ title, onClose, children }: {
  title: string; onClose: () => void; children: React.ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const trigger = document.activeElement as HTMLElement | null;
    const dialog = ref.current;
    dialog?.showModal();
    return () => { dialog?.close(); if (trigger?.isConnected) trigger.focus(); };
  }, []);
  return createPortal(
    <dialog ref={ref} aria-label={title}
      onCancel={e => { e.preventDefault(); onClose(); }}
      onKeyDown={e => {
        if (e.key === 'Escape') { e.preventDefault(); onClose(); }
        if (e.key === 'Tab') {
          const controls = Array.from(e.currentTarget.querySelectorAll<HTMLElement>(':is(button, input, select, textarea, a[href]):not(:disabled)'));
          const first = controls[0];
          const last = controls[controls.length - 1];
          if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last?.focus(); }
          if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first?.focus(); }
        }
      }}
      onClick={e => { if (e.target === e.currentTarget) onClose(); }}
      style={{ ...D.overlay, width: '100%', maxWidth: '100%', height: '100%', maxHeight: '100%', margin: 0, padding: 16, border: 0, boxSizing: 'border-box', color: 'var(--text)' }}>
      <div style={{ ...D.dialog, width: 'min(480px, 100%)', maxHeight: 'calc(100dvh - 32px)', overflowY: 'auto', boxSizing: 'border-box' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
          <div style={{ ...D.dialogTitle, flex: 1, minWidth: 0, overflowWrap: 'anywhere' }}>{title}</div>
          <button type="button" aria-label="Dismiss" style={PD.iconBtn} onClick={onClose}><XIcon /></button>
        </div>
        {children}
      </div>
    </dialog>, document.body,
  );
}

/** Icon-only Play/Shuffle/Queue/Add Tracks row — matches the album/artist
 * header treatment (transparent .icon-action-btn + hover/focus tooltip). */
function PlaylistToolbar({
  disabled, onPlay, onShuffle, onQueue, addTracksOpen, onToggleAddTracks, mixReady, onEditRules,
}: {
  disabled: boolean;
  onPlay: () => void;
  onShuffle: () => void;
  onQueue: () => void;
  addTracksOpen: boolean;
  onToggleAddTracks: () => void;
  /** Set when a finished BoogieMix exists — shows the "mix ready" badge beside Play. */
  mixReady?: { name?: string; onOpen: () => void } | null;
  /** Set for smart playlists: "Edit rules" replaces Add Tracks. */
  onEditRules?: () => void;
}) {
  const mixReadyLabel = mixReady
    ? `BoogieMix ready${mixReady.name ? ` — ${mixReady.name}` : ''} · Open in Mixes`
    : '';
  return (
    <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexShrink: 0 }}>
      <button type="button" className="icon-action-btn icon-action-btn--primary" data-tip="Play All" aria-label="Play All"
        disabled={disabled} onClick={onPlay}>
        <PlayIcon size={15} />
      </button>
      {mixReady && (
        <button type="button" className="icon-action-btn icon-action-btn--ready" data-tip={mixReadyLabel} aria-label={mixReadyLabel}
          onClick={mixReady.onOpen}>
          <MixIcon size={16} />
          <span className="icon-action-btn__badge" aria-hidden="true"><CheckIcon /></span>
        </button>
      )}
      <button type="button" className="icon-action-btn" data-tip="Shuffle Play" aria-label="Shuffle play all tracks"
        disabled={disabled} onClick={onShuffle}>
        <ShuffleIcon />
      </button>
      <button type="button" className="icon-action-btn" data-tip="Queue All" aria-label="Queue all tracks"
        disabled={disabled} onClick={onQueue}>
        <QueueIcon size={17} />
      </button>
      {onEditRules ? (
        <button type="button" className="icon-action-btn" data-tip="Edit rules" aria-label="Edit rules"
          aria-haspopup="dialog" onClick={onEditRules}>
          <SmartIcon size={17} />
        </button>
      ) : (
        <button type="button" className={addTracksOpen ? 'icon-action-btn icon-action-btn--primary' : 'icon-action-btn'}
          data-tip="Add Tracks" aria-label="Add tracks" onClick={onToggleAddTracks}>
          <PlusIcon size={17} />
        </button>
      )}
    </div>
  );
}

// ─── New / Edit Playlist Dialog ───────────────────────────────────────────────

/** Synthesizes a `Track`-shaped object for a finished BoogieMix output so it
 * can be played through the normal Player queue/now-playing flow instead of
 * only being downloadable. The `boogiemix:`-prefixed id keeps it from
 * colliding with real library track ids anywhere `track.id` is used as a DB
 * lookup key; `stream_url_override` is what `getPreferredTrackStreamUrl`
 * picks up in Player.tsx. Title format must stay in sync with the ID3 title
 * `render_mix` stamps into the file itself (mix_worker.rs).
 *
 * `cover_album_ids` carries the mix's full collage snapshot (same source as
 * the Mixes-library collage) so the now-playing bar can render the actual
 * 2x2 collage instead of one album's art; `album_id` is set to the first
 * entry as a single-image fallback for surfaces that only render one image
 * (vinyl turntable, adaptive-accent color extraction, queue-panel rows). */
export function mixOutputToTrack(output: BoogieMixOutput, playlistName: string): Track {
  let coverAlbumIds: ClientEntityId[] = [];
  if (output.cover_album_ids) {
    try {
      const parsed = JSON.parse(output.cover_album_ids);
      if (Array.isArray(parsed)) coverAlbumIds = parsed;
    } catch { /* malformed/legacy row — fall through with no art */ }
  }
  return {
    id: `boogiemix:${output.id}`,
    file_name: output.file_name,
    file_size: output.file_size_bytes ?? null,
    format: output.format,
    duration: output.duration_sec,
    bitrate: null,
    sample_rate: null,
    channels: null,
    title: output.name || `${playlistName} — BoogieMix`,
    artist: 'BoogieBox BoogieMix',
    album: playlistName,
    library_name: null,
    track_number: null,
    disc_number: null,
    year: null,
    genre: null,
    composer: null,
    comment: null,
    bpm: null,
    album_id: coverAlbumIds[0] ?? null,
    cover_album_ids: coverAlbumIds.length > 0 ? coverAlbumIds : null,
    scanned_at: output.created_at,
    stream_url_override: api.boogiemix.playUrl(output.id),
  };
}

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
    <PlaylistPopup title={initial ? 'Rename Playlist' : 'New Playlist'} onClose={onCancel}>
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
    </PlaylistPopup>
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
    <PlaylistPopup title="Delete Playlist" onClose={onCancel}>
        <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.4 }}>
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
    </PlaylistPopup>
  );
}

/** Saves a smart playlist's current tracks as a playlist edited by hand. */
function SaveAsOrdinaryDialog({ playlist, existingNames, onSaved, onCancel }: {
  playlist: Playlist;
  existingNames: string[];
  onSaved: (copy: Playlist) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(`${playlist.name} (copy)`);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => { inputRef.current?.focus(); inputRef.current?.select(); }, []);
  const count = playlist.track_count;
  const submit = async () => {
    const trimmed = name.trim();
    if (!trimmed || saving) return;
    if (existingNames.some(existing => normalizePlaylistName(existing) === normalizePlaylistName(trimmed))) {
      setError('A playlist with this name already exists');
      return;
    }
    setSaving(true);
    try {
      const result = await api.playlists.saveAsOrdinary(playlist.id, trimmed);
      onSaved(result.playlist);
    } catch (e: any) {
      setError(e?.message || 'Could not create playlist');
      setSaving(false);
    }
  };
  return (
    <PlaylistPopup title="Save as ordinary playlist" onClose={onCancel}>
      <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.55 }}>
        Saves these {count === 1 ? '1 track' : `${count.toLocaleString()} tracks`} as a playlist you can edit by hand. It won&rsquo;t change when the rules match other tracks.
      </div>
      <label htmlFor="smart-copy-name" style={{ fontSize: 13, fontWeight: 650, color: 'var(--text-muted)' }}>Playlist name</label>
      <input id="smart-copy-name" ref={inputRef} style={D.input} value={name}
        onChange={e => { setError(''); setName(e.target.value); }}
        onKeyDown={e => { if (e.key === 'Enter') void submit(); }} />
      {error && <div role="alert" style={D.errorText}>{error}</div>}
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        <button type="button" style={D.cancelBtn} onClick={onCancel}>Cancel</button>
        <button type="button" style={{ ...D.saveBtn, ...(!name.trim() || saving ? hybridControlStyles.disabled : {}) }}
          disabled={!name.trim() || saving} onClick={() => void submit()}>
          <CopyIcon /> Save copy
        </button>
      </div>
    </PlaylistPopup>
  );
}

const D: Record<string, React.CSSProperties> = {
  overlay: { position: 'fixed', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 200, ...hybridMediaStyles.overlay },
  dialog: { padding: 24, width: 400, display: 'flex', flexDirection: 'column', gap: 12, ...hybridMediaStyles.dialog },
  dialogTitle: { fontSize: 19, fontWeight: 750, color: 'var(--text)', marginBottom: 4, letterSpacing: -0.3 },
  input: { ...hybridControlStyles.field },
  errorText: { color: 'var(--danger)', fontSize: 14, lineHeight: 1.3 },
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
        <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)' }}>Add Tracks</div>
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
  searchInput: { flex: 1, background: 'transparent', border: 'none', outline: 'none', color: 'var(--text)', fontSize: 14, fontFamily: 'inherit', padding: '2px 0' },
  clearBtn: { background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', display: 'flex', alignItems: 'center', padding: 0 },
  results: { flex: 1, overflowY: 'auto', paddingBottom: 0 },
  hint: { padding: '24px 14px', textAlign: 'center', color: 'var(--text-muted)', fontSize: 14 },
  row: { display: 'flex', alignItems: 'center', gap: 8, margin: '2px 8px', padding: '8px 10px', border: 'none', borderRadius: 10, transition: 'background 0.1s' },
  trackTitle: { fontSize: 14, fontWeight: 500, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  trackSub: { fontSize: 12, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', marginTop: 2 },
  dur: { fontSize: 12, color: 'var(--text-muted)', flexShrink: 0, fontVariantNumeric: 'tabular-nums' },
  addBtn: { ...hybridControlStyles.tonalButton, padding: 0, minHeight: 34, width: 34, height: 34, flexShrink: 0, fontSize: 14 },
};

// ─── Draggable Track Row ──────────────────────────────────────────────────────

function DraggableTrackRow({
  track, index, total,
  onPlay, onRemove, onDragStart, onDragEnter, onDragEnd,
  isDragOver, isHovered, onHoverChange,
  onOpenAlbum, onOpenAlbumArtist, fixed = false,
}: {
  track: PlaylistTrack; index: number; total: number;
  onPlay: () => void; onRemove: () => void;
  onDragStart: (i: number) => void;
  onDragEnter: (i: number) => void;
  onDragEnd: () => void;
  isDragOver: boolean;
  isHovered: boolean;
  onHoverChange: (hovered: boolean) => void;
  onOpenAlbum: (albumId: ClientEntityId) => void;
  onOpenAlbumArtist: (artistId: ClientEntityId) => void;
  /** Smart playlist rows: no dragging and no remove button. */
  fixed?: boolean;
}) {
  // A separate "album artist" link is only worth adding when it actually
  // differs from the track's own artist already shown — e.g. a compilation
  // track credited to its own performer on an album owned by "Various
  // Artists". When they match, the displayed artist name IS the album
  // artist, so it becomes the clickable link itself instead of being plain
  // text with a redundant (and previously: unclickable) link hidden next to it.
  const albumArtistMatchesTrackArtist = !!(
    track.album_artist_id && track.album_artist_name && track.album_artist_name === track.artist
  );
  const showAlbumArtistLink = !!(
    track.album_artist_id && track.album_artist_name && track.album_artist_name !== track.artist
  );
  return (
    <div
      draggable={!fixed}
      onDragStart={fixed ? undefined : () => onDragStart(index)}
      onDragEnter={fixed ? undefined : () => onDragEnter(index)}
      onDragEnd={fixed ? undefined : onDragEnd}
      onDragOver={fixed ? undefined : e => e.preventDefault()}
      onClick={onPlay}
      onMouseEnter={() => onHoverChange(true)}
      onMouseLeave={() => onHoverChange(false)}
      style={{
        ...T.row,
        ...(fixed ? { cursor: 'default' } : {}),
        backgroundColor: isDragOver
          ? 'color-mix(in srgb, var(--accent) 10%, transparent)'
          : isHovered
            ? 'color-mix(in srgb, var(--accent) 12%, transparent)'
            : 'transparent',
        borderTop: isDragOver ? '2px solid var(--accent)' : '2px solid transparent',
      }}
    >
      {!fixed && <div style={T.grip} title="Drag to reorder"><GripIcon /></div>}
      <div style={T.num}>{index + 1}</div>
      <button type="button" aria-label={`Play ${track.title || track.file_name}`} style={T.playBtn} onClick={(e) => { e.stopPropagation(); onPlay(); }} title="Play"><PlayIcon /></button>
      <div style={T.art}>
        {track.album_id ? (
          <ArtImage src={api.albumArtUrl(track.album_id, 300)} alt="" imgStyle={T.artImg} />
        ) : (
          <div style={T.artFallback}><NoteIcon /></div>
        )}
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={T.title}>{track.title || track.file_name}</div>
        <div style={T.sub}>
          {track.artist && albumArtistMatchesTrackArtist ? (
            <button
              type="button"
              className="inline-text-link"
              style={T.subLink}
              onClick={(e) => { e.stopPropagation(); onOpenAlbumArtist(track.album_artist_id!); }}
              title={`Go to artist: ${track.artist}`}
            >
              {track.artist}
            </button>
          ) : track.artist}
          {track.artist && track.album ? ' · ' : ''}
          {track.album && track.album_id ? (
            <button
              type="button"
              className="inline-text-link"
              style={T.subLink}
              onClick={(e) => { e.stopPropagation(); onOpenAlbum(track.album_id!); }}
              title={`Go to album: ${track.album}`}
            >
              {track.album}
            </button>
          ) : track.album}
          {showAlbumArtistLink && ' — '}
          {showAlbumArtistLink && (
            <button
              type="button"
              className="inline-text-link"
              style={T.subLink}
              onClick={(e) => { e.stopPropagation(); onOpenAlbumArtist(track.album_artist_id!); }}
              title={`Go to album artist: ${track.album_artist_name}`}
            >
              {track.album_artist_name}
            </button>
          )}
        </div>
      </div>
      {track.has_deep_analysis && (
        <span style={{ fontSize: 12, color: 'var(--accent)', opacity: 0.55, flexShrink: 0 }} title="Sonic Fingerprint available — AI stem analysis complete">✦</span>
      )}
      <div style={T.dur}>{fmtTrackDur(track.duration)}</div>
      {!fixed && <button type="button" aria-label={`Remove ${track.title || track.file_name} from playlist`} style={T.removeBtn} onClick={(e) => { e.stopPropagation(); onRemove(); }} title="Remove from playlist"><TrashIcon /></button>}
    </div>
  );
}

const T: Record<string, React.CSSProperties> = {
  row: { display: 'flex', alignItems: 'center', gap: 8, cursor: 'grab', userSelect: 'none', transition: 'background 0.1s', ...hybridMediaStyles.listRow },
  grip: { color: 'var(--text-muted)', flexShrink: 0, opacity: 0.35, display: 'flex', alignItems: 'center', cursor: 'grab' },
  num: { width: 25, textAlign: 'right', fontSize: 13, color: 'var(--text-muted)', flexShrink: 0, fontVariantNumeric: 'tabular-nums' },
  playBtn: { background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', padding: '2px 3px', borderRadius: 4, display: 'flex', alignItems: 'center', opacity: 0.55, flexShrink: 0 },
  art: { width: 32, height: 32, borderRadius: 6, overflow: 'hidden', flexShrink: 0, border: '1px solid color-mix(in srgb, var(--text-muted) 20%, var(--border))', backgroundColor: 'var(--surface-subtle)' },
  artImg: { width: '100%', height: '100%', objectFit: 'cover', display: 'block' },
  artFallback: { width: '100%', height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)', opacity: 0.4 },
  title: { fontSize: 14, fontWeight: 500, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  sub: { fontSize: 12, color: 'var(--text-muted)', marginTop: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  subLink: { background: 'none', border: 'none', padding: 0, margin: 0, color: 'var(--accent)', cursor: 'pointer', font: 'inherit', fontSize: 'inherit' },
  dur: { fontSize: 13, color: 'var(--text-muted)', flexShrink: 0, fontVariantNumeric: 'tabular-nums', width: 42, textAlign: 'right' },
  removeBtn: { background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', padding: '2px 4px', borderRadius: 4, display: 'flex', alignItems: 'center', opacity: 0.4, flexShrink: 0 },
};

// ─── Playlist Detail Panel ────────────────────────────────────────────────────


function PlaylistOptions({ playlist, onUpdate, onDelete, onClose, onEditRules, onSaveCopy }: {
  playlist: Playlist; onUpdate: () => void; onDelete: (id: EntityId) => void; onClose: () => void;
  /** Smart playlists: open the rule editor / the "Save as ordinary playlist" dialog. */
  onEditRules?: () => void; onSaveCopy?: () => void;
}) {
  // Keep the original row/toolbar trigger across the nested editor and confirmation views.
  useEffect(() => {
    const trigger = document.activeElement as HTMLElement | null;
    return () => { if (trigger?.isConnected) trigger.focus(); };
  }, []);
  const [showEdit, setShowEdit]   = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [deleteError, setDeleteError] = useState('');
  const [editError, setEditError] = useState('');
  const [cfMode, setCfMode]       = useState<CrossfadeMode>('off');
  const [cfDuration, setCfDuration] = useState(2);
  const [cfHasOverride, setCfHasOverride] = useState(false);
  const [cfSaving, setCfSaving]   = useState(false);
  const [rememberProgress, setRememberProgress] = useState(!!(playlist.remember_progress));
  const [rememberSaving, setRememberSaving] = useState(false);
  useEffect(() => {
    let canceled = false;
    api.crossfade.config('playlist', playlist.id).then(cfg => {
      if (canceled) return;
      setCfMode(cfg.mode); setCfDuration(cfg.duration); setCfHasOverride(cfg.source === 'override');
    }).catch(() => {});
    return () => { canceled = true; };
  }, [playlist.id]);
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
      onDelete(playlist.id);
    } catch (e: any) {
      setDeleteError(e?.message || 'Could not delete playlist');
    }
  };

  const handleToggleRememberProgress = async () => {
    const next = !rememberProgress;
    setRememberSaving(true);
    setEditError('');
    try {
      await api.playlists.update(playlist.id, playlist.name, playlist.description ?? '', next ? 1 : 0);
      setRememberProgress(next);
      onUpdate();
    } catch (e: any) {
      setEditError(e?.message || 'Could not update playlist');
    } finally { setRememberSaving(false); }
  };


  if (showEdit) return <PlaylistDialog initial={{ name: playlist.name, description: playlist.description ?? '' }}
    onSave={handleEdit} onCancel={() => { setEditError(''); setShowEdit(false); }} error={editError} />;
  if (showDeleteConfirm) return <DeletePlaylistDialog playlistName={playlist.name}
    onConfirm={handleDelete} onCancel={() => { setDeleteError(''); setShowDeleteConfirm(false); }} error={deleteError} />;
  return (
    <PlaylistPopup title={playlist.name} onClose={onClose}>
      {playlist.kind === 'smart' && onEditRules && (
        <button type="button" style={PD.menuItem} onClick={onEditRules}><SmartIcon size={12} /> Edit rules</button>
      )}
      {playlist.kind === 'smart' && onSaveCopy && (
        <button type="button" style={PD.menuItem} onClick={onSaveCopy}><CopyIcon /> Save as ordinary playlist</button>
      )}
      <button type="button" title="Rename" style={PD.menuItem} onClick={() => { setEditError(''); setShowEdit(true); }}><EditIcon /> Rename</button>
      <button type="button" role="switch" aria-checked={rememberProgress} aria-label="Remember track position"
        disabled={rememberSaving} title={rememberProgress ? 'Remember track position: On' : 'Remember track position: Off'}
        style={{ ...PD.menuItem, justifyContent: 'space-between' }} onClick={handleToggleRememberProgress}>
        Remember track position
        <span aria-hidden="true" style={{ width: 40, height: 24, borderRadius: 12, padding: 3, boxSizing: 'border-box', background: rememberProgress ? 'var(--accent)' : 'var(--border)' }}>
          <span style={{ display: 'block', width: 18, height: 18, borderRadius: '50%', background: 'var(--text)', transform: rememberProgress ? 'translateX(16px)' : undefined }} />
        </span>
      </button>
      {editError && <div role="alert" style={D.errorText}>{editError}</div>}
        <div style={{ padding: '16px 0', borderTop: '1px solid var(--border)' }}>
          <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 0.8, marginBottom: 8 }}>
            Crossfade settings
          </div>
          {/* Mode pills */}
          <div style={{ ...PD.segmentedGroup, display: 'flex', marginBottom: 8 }}>
            {([
              { value: 'off' as const, label: 'Off' },
              { value: 'zerogap' as const, label: 'Zero-gap' },
              { value: 'crossfade' as const, label: 'Crossfade' },
            ]).map(opt => (
              <button
                key={opt.value}
                aria-pressed={cfMode === opt.value}
                onClick={async () => {
                  setCfMode(opt.value);
                  setCfSaving(true);
                  await api.crossfade.upsertOverride({ entity_type: 'playlist', entity_id: playlist.id, mode: opt.value, duration: cfDuration }).catch(() => {});
                  setCfHasOverride(true);
                  setCfSaving(false);
                }}
                style={{
                  ...PD.segment,
                  flex: 1,
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
              <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>1s</span>
              <input
                type="range" aria-label="Crossfade settings" min={1} max={10} step={1} value={cfDuration}
                onChange={async e => {
                  const v = Number(e.target.value);
                  setCfDuration(v);
                  setCfSaving(true);
                  await api.crossfade.upsertOverride({ entity_type: 'playlist', entity_id: playlist.id, mode: cfMode, duration: v }).catch(() => {});
                  setCfHasOverride(true);
                  setCfSaving(false);
                }}
                style={{ flex: 1, minWidth: 0, accentColor: 'var(--accent)' }}
              />
              <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>10s</span>
              <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)' }}>{cfDuration}s</span>
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
                  fontSize: 12,
                }}
              >
                Reset to default
              </button>
            )}
            {cfSaving && <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>Saving…</span>}
            {!cfHasOverride && !cfSaving && <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>Using global default</span>}
          </div>
        </div>
      <button type="button" title="Delete playlist" style={{ ...PD.menuItem, borderTop: '1px solid var(--border)', color: 'var(--danger)' }}
        onClick={() => { setDeleteError(''); setShowDeleteConfirm(true); }}><TrashIcon /> Delete playlist</button>
    </PlaylistPopup>
  );
}

/** Default mix name — "Mix — Sep 15" — computed client-side, editable before Start. */
function defaultMixName(): string {
  return `Mix — ${new Date().toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}`;
}

function PlaylistDetail({
  playlist, onUpdate, onOptions, onBack,
  playTrack, addToQueue, onOpenAlbum, onOpenArtist, onOpenMixes, onEditRules, reloadToken = 0,
}: {
  playlist: Playlist;
  onUpdate: () => void;
  onOptions: () => void;
  onBack: () => void;
  playTrack: (track: Track, all?: Track[], source?: import('../types').QueueSource) => void;
  addToQueue: (track: Track) => void;
  onOpenAlbum: (album: import('../types').Album) => void;
  onOpenArtist: (artist: import('../types').Artist) => void;
  onOpenMixes: (playlistName: string) => void;
  /** Smart playlists: opens the rule editor. */
  onEditRules?: () => void;
  /** Bumped after the rules change so the tracks load again. */
  reloadToken?: number;
}) {
  const smart = playlist.kind === 'smart';
  const [tracks, setTracks]       = useState<PlaylistTrack[]>([]);
  // Smart playlists: the last refresh failed and the previous result is shown.
  const [refreshError, setRefreshError] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const loadedRef = useRef(false);
  const loadSeqRef = useRef(0);
  const [loading, setLoading]     = useState(true);
  const [loadError, setLoadError] = useState('');
  const [showAdd, setShowAdd]     = useState(false);
  const [dragFrom, setDragFrom]   = useState<number | null>(null);
  const [dragOver, setDragOver]   = useState<number | null>(null);
  const rememberProgress = !!playlist.remember_progress;
  const [showMix, setShowMix] = useState(false);
  // High Quality now defaults on, so the "needs deep analysis" fallback
  // warning would otherwise show on the main status line for every playlist
  // on any machine lacking deep-analysis deps, even if BoogieMix was never
  // opened. Gate it behind having opened the popup at least once this
  // session, so it still greets the user there before they hit Start, but
  // doesn't become an ambient warning across the whole app.
  const [hasOpenedMix, setHasOpenedMix] = useState(false);
  const [mixStarting, setMixStarting] = useState(false);
  const [hoveredTrackId, setHoveredTrackId] = useState<ClientEntityId | null>(null);
  const [mixJobId, setMixJobId] = useState<ClientEntityId | null>(null);
  const [mixJob, setMixJob] = useState<BoogieMixJob | null>(null);
  const [mixOutputs, setMixOutputs] = useState<any[]>([]);
  const [mixError, setMixError] = useState('');
  const [mixStyle, setMixStyle] = useState<'chill_blend' | 'club_blend' | 'long_build' | 'safe_mix'>('club_blend');
  const [mixQuality, setMixQuality] = useState<'standard' | 'high_quality'>('high_quality');
  const [mixCrossfade, setMixCrossfade] = useState(16);
  const [mixOrderMode, setMixOrderMode] = useState<'style' | 'playlist'>('style');
  const [mixName, setMixName] = useState(defaultMixName());
  const [deepStatus, setDeepStatus] = useState<BoogieMixDeepAnalysisStatus | null>(null);
  const [deepRunning, setDeepRunning] = useState(false);
  const [deepProgress, setDeepProgress] = useState<PlaylistDeepAnalysisProgress | null>(null);
  const [deepQueuedCount, setDeepQueuedCount] = useState(0);
  const [deepError, setDeepError] = useState('');
  const [statusHover, setStatusHover] = useState(false);
  // Shown instead of the mix config in the popup when Start is clicked on a
  // playlist with no deep-analyzed tracks at all — offers to analyze first.
  const [showAnalyzeWarning, setShowAnalyzeWarning] = useState(false);
  // Tracks the last-seen completed count (done + skipped) so the poll below
  // only re-fetches tracks when a track's deep-analysis status actually
  // changed, instead of on every 2s tick.
  const deepCompletedRef = useRef(0);

  const applySnapshot = useCallback((snapshot: PlaylistSnapshot) => {
    setTracks(snapshot.tracks);
    setRefreshError(!!snapshot.refreshError);
  }, []);

  const loadTracks = useCallback(async () => {
    const seq = ++loadSeqRef.current;
    // A smart playlist that is already showing tracks updates in place.
    const inPlace = smart && loadedRef.current;
    if (inPlace) setRefreshing(true); else setLoading(true);
    setLoadError('');
    try {
      if (smart) {
        const snapshot = await api.playlists.snapshot(playlist.id);
        if (seq !== loadSeqRef.current) return;
        applySnapshot(snapshot);
      } else {
        const t = await api.playlists.tracks(playlist.id);
        if (seq !== loadSeqRef.current) return;
        setTracks(t);
      }
      loadedRef.current = true;
    } catch (e: any) {
      if (seq !== loadSeqRef.current) return;
      if (inPlace) setRefreshError(true);
      else {
        setTracks([]);
        setLoadError(e?.message || 'Could not load playlist tracks');
      }
    } finally {
      if (seq === loadSeqRef.current) { setLoading(false); setRefreshing(false); }
    }
  }, [playlist.id, smart, applySnapshot]);

  useEffect(() => { loadTracks(); }, [loadTracks, reloadToken]);

  const retryRefresh = async () => {
    const seq = ++loadSeqRef.current;
    setRefreshing(true);
    try {
      const snapshot = await api.playlists.refresh(playlist.id);
      if (seq !== loadSeqRef.current) return;
      applySnapshot(snapshot);
      onUpdate();
    } catch {
      if (seq === loadSeqRef.current) setRefreshError(true);
    } finally {
      if (seq === loadSeqRef.current) setRefreshing(false);
    }
  };
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

  // Reattach to a still-running BoogieMix job on mount, so leaving this
  // playlist (or the page) and coming back keeps showing live progress
  // instead of losing it to the component remount.
  useEffect(() => {
    if (!api.boogiemix?.latestJobForPlaylist) return;
    let canceled = false;
    api.boogiemix.latestJobForPlaylist(playlist.id).then(job => {
      if (canceled || !job || !MIX_ACTIVE_STATUSES.includes(job.status)) return;
      setMixJob(job);
      setMixJobId(job.id);
    }).catch(() => {});
    return () => { canceled = true; };
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

  const remove = async (trackId: ClientEntityId) => {
    await api.playlists.removeTrack(playlist.id, trackId);
    setTracks(prev => prev.filter(t => t.id !== trackId));
    onUpdate();
  };

  const handleOpenAlbum = async (albumId: ClientEntityId) => {
    try {
      const album = await api.album(albumId);
      onOpenAlbum(album);
    } catch {
      // Ignore lookup failures — the row just stays put.
    }
  };
  const handleOpenAlbumArtist = async (artistId: ClientEntityId) => {
    try {
      const artist = await api.artist(artistId);
      onOpenArtist(artist);
    } catch {
      // Ignore lookup failures — the row just stays put.
    }
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

  const playAll   = () => { if (tracks.length) playTrack(tracks[0], tracks, { type: 'playlist', id: playlist.id, rememberProgress }); };
  const shufflePlay = () => {
    if (!tracks.length) return;
    const shuffled = [...tracks];
    for (let i = shuffled.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
    }
    playTrack(shuffled[0], shuffled, { type: 'playlist', id: playlist.id, rememberProgress });
  };
  const queueAll  = () => tracks.forEach(t => addToQueue(t));
  const totalDur  = tracks.reduce((a, t) => a + (t.duration ?? 0), 0);
  const collageAlbumIds = buildPlaylistCollageAlbumIds(tracks);
  const headerCollageAlbumIds = playlist.art_album_ids ?? collageAlbumIds;
  const updatedDate = parseServerDate(playlist.updated_at);
  const deepFallbackMessage = formatBoogieMixFallbackMessage(
    mixJob,
    mixQuality === 'high_quality' && hasOpenedMix ? deepStatus : null,
  );
  const analyzedTrackCount = tracks.filter(t => t.has_deep_analysis).length;
  const playlistNotAnalyzed = tracks.length > 0 && analyzedTrackCount === 0;
  const usedDeepAnalysis = Boolean(mixJob?.used_deep_analysis);
  const planSummary = mixJob?.plan_summary;
  const energyCurvePhases = planSummary?.energyCurvePhases ?? [];
  const mixActive = !!(mixJob && MIX_ACTIVE_STATUSES.includes(mixJob.status));
  const mixReady = !mixActive && (mixJob?.status === 'done' || !!mixOutputs[0]);
  // Rendering can't be canceled mid-render (only the pre-render analyze/plan steps can).
  const mixCancelable = !!(mixJob && ['pending', 'analyzing', 'planning'].includes(mixJob.status));
  const deepDoneCount = (deepProgress?.done ?? 0) + (deepProgress?.skipped ?? 0);
  const deepTotalCount = deepProgress?.total ?? 0;
  const deepActive = deepRunning || !!(deepProgress && deepProgress.running > 0);

  // Collapses every BoogieMix/deep-analysis signal into the single most
  // relevant line — full detail lives in the hover popover below.
  let statusLine: { tone: 'active' | 'done' | 'error' | 'warn'; text: string } | null = null;
  if (mixActive) {
    const step = mixJob!.current_step || MIX_STEP_LABEL[mixJob!.status] || mixJob!.status;
    statusLine = { tone: 'active', text: `BoogieMix — ${step} ${mixJob!.progress_percent ?? 0}%` };
  } else if (deepActive) {
    statusLine = {
      tone: 'active',
      text: deepProgress
        ? `Deep analysis — ${deepDoneCount}/${deepTotalCount} tracks`
        : `Deep analysis — queuing ${deepQueuedCount} tracks`,
    };
  } else if (deepError) {
    // Checked ahead of a stale mix error: deep analysis is the action the
    // user just took, so its failure is the more relevant one to surface.
    statusLine = { tone: 'error', text: deepError };
  } else if (mixJob?.status === 'failed' || mixError) {
    statusLine = { tone: 'error', text: mixError || 'BoogieMix failed' };
  } else if (mixJob?.status === 'canceled') {
    statusLine = { tone: 'warn', text: 'BoogieMix canceled' };
  } else if (mixReady) {
    // A finished mix is surfaced by the toolbar badge, not a status line.
  } else if (deepProgress) {
    statusLine = { tone: 'done', text: `Deep analysis complete — ${deepDoneCount}/${deepTotalCount} tracks` };
  } else if (deepFallbackMessage) {
    statusLine = { tone: 'warn', text: deepFallbackMessage };
  }
  const statusToneColor = statusLine
    ? { active: 'var(--text)', done: 'var(--success)', error: 'var(--danger)', warn: 'var(--warning)' }[statusLine.tone]
    : 'var(--text)';
  const startBoogieMix = async () => {
    if (mixStarting) return;
    setMixStarting(true);
    try {
      if (!api.boogiemix) return;
      setMixError('');
      const created = await api.boogiemix.createJob(playlist.id, mixStyle, mixQuality, mixCrossfade, mixOrderMode, mixName);
      setMixJobId(created.jobId);
      setMixJob(await api.boogiemix.getJob(created.jobId));
    } catch (e: any) {
      setMixError(e?.message || 'Failed to start BoogieMix');
    } finally {
      setMixStarting(false);
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
    deepCompletedRef.current = 0;
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
    const poll = async () => {
      try {
        const prog = await api.boogiemix!.playlistDeepAnalysisProgress(playlist.id);
        if (stopped) return;
        setDeepProgress(prog);
        // A track finished (or was skipped) since the last tick — refresh the
        // track list so its Sonic Fingerprint icon shows up without the user
        // having to leave and reopen the playlist.
        const completed = prog.done + prog.skipped;
        if (completed > deepCompletedRef.current) {
          deepCompletedRef.current = completed;
          api.playlists.tracks(playlist.id).then(fresh => { if (!stopped) setTracks(fresh); }).catch(() => {});
        }
        if (prog.pending === 0 && prog.running === 0) {
          clearInterval(timer);
          setDeepRunning(false);
        }
      } catch {}
    };
    const timer = setInterval(poll, 2000);
    // Backgrounded tabs throttle setInterval, so refresh immediately on refocus
    // instead of waiting for the throttled tick to catch up.
    const onVisibilityChange = () => { if (document.visibilityState === 'visible') poll(); };
    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => {
      stopped = true;
      clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [deepRunning, playlist.id]);

  return (
    <div data-ui-region="playlist-detail" style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0, overflow: 'hidden' }}>
      {/* Header: the shared single-row command bar (same as Browse/Search). */}
      <CommandBar
        watermark="Playlists"
        style={PD.bar}
        actions={(
          <>
            <button
              type="button"
              aria-haspopup="dialog"
              style={PD.iconBtn}
              onClick={() => { setShowAnalyzeWarning(false); setShowMix(true); setHasOpenedMix(true); setMixName(defaultMixName()); }}
              title="BoogieMix (Experimental) — AI-planned transitions for this playlist"
              aria-label="BoogieMix (Experimental)"
            >
              <MixIcon />
            </button>
            <PlaylistKebab onClick={onOptions} />
          </>
        )}
      >
        <button type="button" style={PD.backBtn} onClick={onBack} aria-label="Back to Playlists" title="Back to Playlists">
          <ChevronLeftIcon /> Playlists
        </button>
        <div aria-label={`${playlist.name} artwork`} style={{ flexShrink: 0 }}><PlaylistArtwork albumIds={headerCollageAlbumIds} thumb /></div>
        <div style={PD.titleBlock} title={playlist.description || undefined}>
          <div style={PD.name}>{playlist.name}</div>
        </div>
        <PlaylistToolbar
          disabled={!tracks.length}
          mixReady={mixReady ? { name: mixOutputs[0]?.name, onOpen: () => onOpenMixes(playlist.name) } : null}
          onPlay={playAll}
          onShuffle={shufflePlay}
          onQueue={queueAll}
          addTracksOpen={showAdd}
          onToggleAddTracks={() => setShowAdd(value => !value)}
          onEditRules={smart ? onEditRules : undefined}
        />
      </CommandBar>
      {showMix && showAnalyzeWarning && (
        <PlaylistPopup title="BoogieMix (Experimental)" onClose={() => { setShowAnalyzeWarning(false); setShowMix(false); }}>
          <div style={{ fontSize: 14, color: 'var(--warning)', fontWeight: 650 }}>
            This playlist hasn&rsquo;t been deep-analyzed yet.
          </div>
          <div style={{ fontSize: 14, color: 'var(--text-muted)' }}>
            BoogieMix plans its transitions from each track&rsquo;s Sonic Fingerprint (AI stem analysis).
            Without it, this mix will fall back to standard analysis and may blend less precisely.
            Run deep analysis first, or continue anyway.
          </div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
            <button
              type="button"
              style={PD.btnPrimary}
              onClick={async () => { setShowAnalyzeWarning(false); await runDeepAnalysis(); setShowMix(false); }}
            >
              ⚡ Run Deep Analysis First
            </button>
            <button
              type="button"
              style={PD.btnSecondary}
              onClick={async () => { setShowAnalyzeWarning(false); await startBoogieMix(); setShowMix(false); }}
            >
              Continue Without Analysis
            </button>
          </div>
        </PlaylistPopup>
      )}
      {showMix && !showAnalyzeWarning && (
        <PlaylistPopup title="BoogieMix (Experimental)" onClose={() => setShowMix(false)}>
          <div style={{ fontSize: 14, color: 'var(--text-muted)' }}>BoogieMix is experimental and may produce inconsistent results.</div>
          {deepFallbackMessage && <div style={{ fontSize: 14, color: 'var(--warning)' }}>{deepFallbackMessage}</div>}
          <label style={{ display: 'flex', flexDirection: 'column', gap: 8, fontSize: 14, color: 'var(--text-muted)' }}>
            <span id="boogiemix-name-label">Mix name</span>
            <input
              type="text"
              value={mixName}
              onChange={(e) => setMixName(e.target.value)}
              placeholder={defaultMixName()}
              aria-labelledby="boogiemix-name-label"
              aria-describedby="boogiemix-name-hint"
              style={PD.select}
            />
            <span id="boogiemix-name-hint" style={{ fontSize: 12, color: 'var(--text-muted)' }}>
              Example default: &ldquo;Mix&rdquo; + today&rsquo;s date. You can rename it later from Mixes.
            </span>
          </label>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, fontSize: 14, color: 'var(--text-muted)' }}>
            <span id="boogiemix-order-mode-label">Track order</span>
            <div role="group" aria-labelledby="boogiemix-order-mode-label" style={{ ...PD.segmentedGroup, display: 'flex' }}>
              {([
                { value: 'style' as const, label: 'Style-based', icon: <ShuffleIcon /> },
                { value: 'playlist' as const, label: 'Playlist order', icon: <ListIcon /> },
              ]).map(opt => (
                <button
                  key={opt.value}
                  type="button"
                  aria-pressed={mixOrderMode === opt.value}
                  onClick={() => setMixOrderMode(opt.value)}
                  style={{
                    ...PD.segment,
                    flex: 1,
                    display: 'inline-flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    gap: 6,
                    ...(mixOrderMode === opt.value ? PD.segmentActive : {}),
                  }}
                >
                  {opt.icon}
                  {opt.label}
                </button>
              ))}
            </div>
          </div>
          {mixOrderMode === 'playlist' && (
            <div style={{ fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.5 }}>
              Order locked to playlist. BoogieMix will not reorder or AI-plan the sequence — it mixes all {tracks.length} tracks exactly as listed.
            </div>
          )}
          <label style={{ display: 'flex', flexDirection: 'column', gap: 8, fontSize: 14, color: 'var(--text-muted)' }}>
            {mixOrderMode === 'playlist' ? 'Transition style' : 'BoogieMix style'}
            <select
            value={mixStyle}
            onChange={(e) => setMixStyle(e.target.value as any)}
            disabled={mixOrderMode === 'playlist'}
            style={{ ...PD.select, minWidth: 120, ...(mixOrderMode === 'playlist' ? hybridControlStyles.disabled : {}) }}
            title={mixOrderMode === 'playlist' ? 'Transition style' : 'BoogieMix style'}
          >
            <option value="chill_blend">Chill blend</option>
            <option value="club_blend">Club blend</option>
            <option value="long_build">Long build</option>
            <option value="safe_mix">Safe mix</option>
          </select>
          </label>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, fontSize: 14, color: 'var(--text-muted)' }}>
            <label htmlFor="boogiemix-quality-select">BoogieMix quality</label>
            <select
            id="boogiemix-quality-select"
            value={mixQuality}
            onChange={(e) => setMixQuality(e.target.value as any)}
            style={{ ...PD.select, minWidth: 164 }}
            title="BoogieMix quality"
            aria-describedby="boogiemix-quality-hint"
          >
            <option value="standard">Standard</option>
            <option value="high_quality">High Quality (Deep Analysis)</option>
          </select>
            <span id="boogiemix-quality-hint" style={{ fontSize: 12, color: 'var(--text-muted)' }}>Standard won&rsquo;t wait for deep analysis, so unanalyzed tracks skip its extra precision.</span>
          </div>
          <label style={{ display: 'flex', flexDirection: 'column', gap: 8, fontSize: 14, color: 'var(--text-muted)' }}>
            Transition length
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
          </label>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
          <button
            type="button"
            style={{
              ...PD.btnPrimary,
              ...(!api.boogiemix || tracks.length < 2 || mixActive || mixStarting
                ? hybridControlStyles.disabled
                : {}),
            }}
            onClick={async () => {
              if (playlistNotAnalyzed) { setShowAnalyzeWarning(true); return; }
              await startBoogieMix();
              setShowMix(false);
            }}
            disabled={!api.boogiemix || tracks.length < 2 || mixActive || mixStarting}
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
            onClick={async () => { await runDeepAnalysis(); setShowMix(false); }}
            disabled={!api.boogiemix || tracks.length === 0 || deepRunning}
            title="Run Demucs deep analysis on all tracks in this playlist. Replaces synthetic placeholder data with real AI stem analysis."
          >
            ⚡ Deep Analysis
          </button>
          </div>
        </PlaylistPopup>
      )}

      {/* Body: tracks + optional add panel */}
      <div style={{ flex: 1, display: 'flex', minHeight: 0, overflow: 'hidden' }}>
        {/* Track list */}
        <div style={{ flex: 1, overflowY: 'auto', paddingBottom: 0 }}>
          <div data-testid="playlist-details" style={PD.details}>
            {smart && (
              <span style={PD.smartChip} title={'Smart playlist\nIts tracks are picked by rules and update on their own'}>
                <SmartIcon size={13} />Smart playlist
              </span>
            )}
            {!loading && (
              <>
                <span style={PD.detailChip}><span style={PD.detailIcon}><QueueIcon size={14} /></span>{tracks.length === 1 ? '1 track' : `${tracks.length} tracks`}</span>
                {totalDur > 0 && <span style={PD.detailChip}><span style={PD.detailIcon}><ClockIcon /></span>{fmtDur(totalDur)}</span>}
                {api.boogiemix && tracks.length > 0 && (
                  <span style={PD.detailChip} title="Tracks with a Sonic Fingerprint (deep analysis)">
                    <span style={PD.detailIcon}><SparkleIcon /></span>{analyzedTrackCount}/{tracks.length} deep-analyzed
                  </span>
                )}
                {updatedDate && <span style={PD.detailChip}><span style={PD.detailIcon}><CalendarIcon /></span>Updated {updatedDate.toLocaleDateString()}</span>}
              </>
            )}
            {refreshing && (
              <span role="status" style={{ ...PD.detailChip, color: 'var(--text-muted)' }}><SpinnerIcon />Updating…</span>
            )}
            {statusLine && !loading && <span style={PD.detailDivider} aria-hidden="true" />}
            {statusLine && (
            <div
              data-testid="boogiemix-status"
              style={PD.statusPopoverWrap}
              onMouseEnter={() => setStatusHover(true)}
              onMouseLeave={() => setStatusHover(false)}
            >
              <div style={PD.statusLine} title={statusLine.text}>
                {statusLine.tone === 'active' && <SpinnerIcon />}
                {statusLine.tone === 'done' && <CheckIcon />}
                {(statusLine.tone === 'error' || statusLine.tone === 'warn') && <AlertIcon />}
                <span style={{ fontSize: 13, fontWeight: 600, color: statusToneColor, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {statusLine.text}
                </span>
                {mixCancelable && (
                  <button style={{ ...PD.btnSecondary, padding: '2px 8px', fontSize: 13 }} onClick={cancelBoogieMix}>
                    Cancel
                  </button>
                )}
              </div>
              {statusHover && (
                <div style={PD.statusPopover}>
                  <div style={{ fontSize: 13, color: 'var(--warning)', fontWeight: 650, marginBottom: 6 }}>
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
                    <div style={{ fontSize: 13, color: 'var(--text-muted)' }}>
                      Quality: {mixJob.mix_quality === 'high_quality' ? 'High Quality (Deep Analysis)' : 'Standard'}
                      {usedDeepAnalysis ? ' · Demucs-enhanced planning used' : ' · Standard analysis path'}
                    </div>
                  )}
                  {deepFallbackMessage && (
                    <div style={{ marginTop: 2, fontSize: 13, color: 'var(--warning)' }}>
                      {deepFallbackMessage}
                    </div>
                  )}
                  {deepStatus && (
                    <div style={{ marginTop: 2, fontSize: 13, color: 'var(--text-muted)' }}>
                      Deep analysis runtime: {deepStatus.runtime?.summary ?? (deepStatus.runtime?.enabled ? 'Ready' : 'Unavailable')} · Queue {deepStatus.queue?.pending ?? 0} pending
                      {mixJob?.deep_analysis_total_count ? ` · Ready ${mixJob.deep_analysis_ready_count ?? 0}/${mixJob.deep_analysis_total_count}` : ''}
                    </div>
                  )}
                  {mixJob?.mix_strategy && (
                    <div style={{ marginTop: 4, fontSize: 13, color: 'var(--text-muted)' }}>
                      AI Mix Strategy: {mixJob.mix_strategy}
                    </div>
                  )}
                  {energyCurvePhases.length > 0 && (
                    <div style={{ marginTop: 4, fontSize: 13, color: 'var(--text-muted)' }}>
                      {energyCurvePhases.join(' → ')}
                    </div>
                  )}
                  {planSummary?.anthemTrackId && (
                    <div style={{ marginTop: 2, fontSize: 13, color: 'var(--text-muted)' }}>
                      Anthem Track ID: {planSummary.anthemTrackId}
                    </div>
                  )}
                  {mixJob?.last_message && <div style={{ marginTop: 4, fontSize: 13, color: 'var(--text-muted)' }}>{mixJob.last_message}</div>}
                  {mixError && <div style={{ marginTop: 4, fontSize: 13, color: 'var(--danger)' }}>{mixError}</div>}
                </div>
              )}
            </div>
            )}
          </div>
          {smart && refreshError && !loading && (
            <div role="alert" style={PD.refreshAlert}>
              <AlertIcon />
              <span style={{ flex: 1 }}>Couldn&rsquo;t update this playlist. Showing the last result.</span>
              <button type="button" style={{ ...PD.btnSecondary, minHeight: 34, padding: '6px 14px', fontSize: 13 }}
                disabled={refreshing} onClick={() => void retryRefresh()}>Try again</button>
            </div>
          )}
          {loading && <div style={PD.empty}>Loading…</div>}
          {!loading && loadError && (
            <div style={PD.empty}>
              <ListIcon />
              <div style={{ marginTop: 12, fontSize: 16, fontWeight: 500, color: 'var(--text)' }}>Could not load tracks</div>
              <div style={{ marginTop: 6, fontSize: 14, color: 'var(--text-muted)' }}>{loadError}</div>
            </div>
          )}
          {smart && !loading && !loadError && tracks.length === 0 && (
            <div role="status" style={PD.empty}>
              <SmartIcon size={22} />
              <div style={{ marginTop: 12, fontSize: 16, fontWeight: 500, color: 'var(--text)' }}>No tracks match these rules.</div>
              {onEditRules && (
                <button type="button" style={{ ...PD.btnPrimary, marginTop: 14 }} onClick={onEditRules}>
                  <SmartIcon size={15} /> Edit rules
                </button>
              )}
            </div>
          )}
          {!smart && !loading && !loadError && tracks.length === 0 && (
            <div style={PD.empty}>
              <ListIcon />
              <div style={{ marginTop: 12, fontSize: 16, fontWeight: 500, color: 'var(--text)' }}>It’s Oh So Quiet</div>
              <div style={{ marginTop: 6, fontSize: 14, color: 'var(--text-muted)' }}>Click "Add Tracks" to search and add music</div>
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
              onOpenAlbum={handleOpenAlbum}
              onOpenAlbumArtist={handleOpenAlbumArtist}
              fixed={smart}
            />
          ))}
        </div>

        {/* Add tracks slide-in panel */}
        {showAdd && !smart && (
          <AddTracksPanel
            playlistId={playlist.id}
            existingTrackIds={new Set(tracks.map(t => t.id))}
            onAdded={() => { loadTracks(); onUpdate(); }}
            onClose={() => setShowAdd(false)}
          />
        )}
      </div>


    </div>
  );
}

const PD: Record<string, React.CSSProperties> = {
  backBtn: {
    display: 'flex', alignItems: 'center', gap: 6, padding: '4px 2px', border: 'none', background: 'transparent',
    color: 'var(--text-muted)', fontSize: 13, fontWeight: 600, fontFamily: 'inherit', cursor: 'pointer', flexShrink: 0,
  },
  bar: { padding: '8px 28px' },
  titleBlock: { minWidth: 0, flex: '0 1 auto', lineHeight: 1.2 },
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
  name: { color: 'var(--text)', fontSize: 20, fontWeight: 800, letterSpacing: -0.3, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  details: { display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: '6px 18px', margin: '10px 16px', minHeight: 22 },
  detailChip: { display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 13, fontWeight: 600, color: 'var(--text)', whiteSpace: 'nowrap' },
  detailIcon: { display: 'inline-flex', color: 'var(--accent)', opacity: 0.85 },
  detailDivider: { width: 1, height: 16, background: 'var(--divider-subtle)' },
  smartChip: {
    display: 'inline-flex', alignItems: 'center', gap: 6, padding: '3px 10px', borderRadius: 999, whiteSpace: 'nowrap',
    background: 'var(--accent-soft)', border: '1px solid color-mix(in srgb, var(--accent) 40%, transparent)',
    color: 'var(--accent)', fontSize: 12, fontWeight: 700,
  },
  refreshAlert: {
    display: 'flex', alignItems: 'center', gap: 10, margin: '0 16px 10px', padding: '10px 14px', borderRadius: 10, fontSize: 14,
    border: '1px solid color-mix(in srgb, var(--danger) 45%, var(--border))', background: 'color-mix(in srgb, var(--danger) 8%, transparent)',
    color: 'var(--text)',
  },
  empty: { ...phase2.desktopMediaRow, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', margin: 24, padding: '72px 20px', color: 'var(--text-muted)', fontSize: 15, ...hybridMediaStyles.emptyState },
  menuItem: { ...hybridControlStyles.secondaryButton, width: '100%', justifyContent: 'flex-start', minHeight: 44, border: 'none', borderRadius: 6, background: 'transparent', textAlign: 'left' },
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
  statusPopoverWrap: { position: 'relative', minWidth: 0 },
  statusLine: { display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 },
  statusPopover: {
    position: 'absolute',
    top: '100%',
    left: 0,
    marginTop: 6,
    minWidth: 280,
    maxWidth: 420,
    zIndex: 20,
    padding: '10px 12px',
    borderRadius: 10,
    border: '1px solid var(--divider-subtle)',
    background: 'var(--surface)',
    boxShadow: '0 8px 24px color-mix(in srgb, black 24%, transparent)',
  },
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
        <span style={{ fontSize: 13, fontWeight: 700, color: 'var(--text)' }}>
          {running ? '⚡ Deep Analysis Running' : finished ? '✓ Deep Analysis Complete' : '⚡ Deep Analysis'}
        </span>
        <button
          onClick={onDismiss}
          style={{ marginLeft: 'auto', background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-muted)', fontSize: 15, lineHeight: 1, padding: '0 2px' }}
          title="Dismiss"
        >
          ×
        </button>
      </div>
      {error && <div style={{ fontSize: 13, color: 'var(--danger)', marginBottom: 4 }}>{error}</div>}
      {progress && (
        <>
          <div style={{ width: '100%', height: 6, borderRadius: 3, backgroundColor: 'var(--border)', overflow: 'hidden', marginBottom: 6 }}>
            <div style={{ height: '100%', width: `${pct}%`, borderRadius: 3, backgroundColor: finished ? 'var(--success)' : 'var(--accent)', transition: 'width 0.4s ease' }} />
          </div>
          <div style={{ fontSize: 13, color: 'var(--text-muted)', display: 'flex', gap: 12, flexWrap: 'wrap' }}>
            <span>{done}/{total} processed</span>
            {active > 0 && <span style={{ color: 'var(--accent)' }}>{active} running</span>}
            {pending > 0 && <span>{pending} queued</span>}
            <span>{analyzedCached} cached</span>
            <span style={{ color: 'var(--success)' }}>{analyzedReal} with real analysis</span>
            {analyzedFallback > 0 && <span style={{ color: 'var(--warning)' }}>{analyzedFallback} fallback</span>}
            {progress.failed > 0 && <span style={{ color: 'var(--danger)' }}>{progress.failed} failed</span>}
          </div>
          {running && active > 0 && (
            <div style={{ fontSize: 12, color: 'var(--text-muted)', marginTop: 4 }}>
              Processing with Demucs CPU analysis — this may take several minutes per track.
            </div>
          )}
          {finished && (
            <div style={{ fontSize: 13, color: 'var(--success)', marginTop: 4 }}>
              {analyzedCached} track{analyzedCached !== 1 ? 's' : ''} have saved deep-analysis data.
              {analyzedFallback > 0 ? ` ${analyzedFallback} are fallback rows; real Demucs stem analysis did not complete for those tracks.` : ''}
              {progress.failed > 0 ? ` ${progress.failed} failed.` : ''}
            </div>
          )}
        </>
      )}
      {!progress && running && (
        <div style={{ fontSize: 13, color: 'var(--text-muted)' }}>
          Queued {queuedCount} track{queuedCount !== 1 ? 's' : ''} for analysis…
        </div>
      )}
    </div>
  );
}

// ─── Playlists Grid (full-page browse, like Browse's album grid) ─────────────

function PlaylistsGridSearchIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  );
}

function PlaylistsCountIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={{ color: 'var(--accent)', opacity: 0.85, flexShrink: 0 }}>
      <path d="M3 6h12M3 12h12M3 18h7" />
      <path d="M17 18V9l4-1" />
      <circle cx="15.5" cy="18" r="1.5" />
    </svg>
  );
}

function PlaylistsGrid({
  playlists, onOpen, onOptions, onCreate, onCreateSmart, onImport,
}: {
  playlists: Playlist[];
  onOpen: (p: Playlist) => void;
  onOptions: (p: Playlist) => void;
  onCreate: () => void;
  onCreateSmart: () => void;
  onImport: (file: File) => void;
}) {
  const [search, setSearch] = useState('');
  const importInputRef = useRef<HTMLInputElement | null>(null);
  const query = search.trim().toLowerCase();
  const filtered = query ? playlists.filter(pl => pl.name.toLowerCase().includes(query)) : playlists;
  const countLabel = `${playlists.length.toLocaleString()} playlist${playlists.length !== 1 ? 's' : ''}`;

  return (
    <div data-ui-region="playlists-grid" style={PG.root}>
      <CommandBar
        watermark="Playlists"
        style={PG.bar}
        actions={(
          <>
            <button
              type="button"
              aria-label="Import playlist"
              style={PG.importBtn}
              onClick={() => importInputRef.current?.click()}
              title={'Import playlist\nFrom an .m3u or .m3u8 file'}
            >
              <ImportIcons.ImportFile size={17} />
            </button>
            <input
              ref={importInputRef}
              type="file"
              accept={PLAYLIST_FILE_ACCEPT}
              hidden
              data-testid="playlist-import-input"
              onChange={(event) => {
                const file = event.target.files?.[0];
                event.target.value = '';
                if (file) onImport(file);
              }}
            />
            <button type="button" aria-label="New smart playlist" aria-haspopup="dialog" style={PG.importBtn}
              onClick={onCreateSmart} title={'New smart playlist\nPicks tracks by rules you set'}>
              <SmartIcon size={17} />
            </button>
            <button type="button" aria-label="New playlist" style={PG.newBtn} onClick={onCreate} title="New playlist">
              <PlusIcon size={16} />
            </button>
          </>
        )}
      >
        <div style={PG.searchBox}>
          <PlaylistsGridSearchIcon />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search playlists"
            style={PG.searchInput}
            aria-label="Search playlists"
          />
        </div>
        <span style={PG.count} title={countLabel} aria-label={countLabel} data-testid="playlists-count">
          <PlaylistsCountIcon />
          <span aria-hidden="true">{playlists.length.toLocaleString()}</span>
        </span>
      </CommandBar>

      <div style={PG.body}>
        {playlists.length === 0 && (
          <div style={PG.empty}>
            <div>It's lonely here. Click + to create a new playlist.</div>
            <button type="button" style={{ ...PD.btnPrimary, marginTop: 14 }} onClick={onCreate}>
              <PlusIcon /> New Playlist
            </button>
          </div>
        )}
        {playlists.length > 0 && (
          <div style={PG.grid}>
            {filtered.map(pl => {
              const smart = pl.kind === 'smart';
              const metaText = [
                smart ? 'Smart' : '',
                `${pl.track_count} track${pl.track_count !== 1 ? 's' : ''}`,
                pl.total_duration ? fmtDur(pl.total_duration) : '',
              ].filter(Boolean).join(' · ');
              const ariaMeta = smart ? metaText.replace(/^Smart/, 'Smart playlist') : metaText;
              return (
                <div key={pl.id} style={PG.card} className="home-list-hover">
                  <button
                    type="button"
                    style={PG.cardBtn}
                    onClick={() => onOpen(pl)}
                    aria-label={`${pl.name} — ${ariaMeta}`}
                    title={`Open ${pl.name}`}
                  >
                    <span aria-hidden="true" style={{ ...PG.art, position: 'relative' }}>
                      <PlaylistArtwork albumIds={pl.art_album_ids ?? []} responsive />
                      {smart && <span data-testid="smart-badge" style={PG.smartBadge}><SmartIcon size={15} /></span>}
                    </span>
                    <span style={PG.cardTitle}>{pl.name}</span>
                    <span style={PG.cardMeta}>{metaText}</span>
                  </button>
                  <div style={PG.cardKebabWrap}>
                    <PlaylistKebab onClick={() => onOptions(pl)} />
                  </div>
                </div>
              );
            })}
          </div>
        )}
        {playlists.length > 0 && filtered.length === 0 && (
          <div style={PG.empty}>No playlists match &quot;{search}&quot;</div>
        )}
      </div>
    </div>
  );
}

const PG: Record<string, React.CSSProperties> = {
  root: { flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', minHeight: 0 },
  // Same single-row command bar as Browse/Search; the grid scrolls beneath it.
  bar: { padding: '8px 28px' },
  body: { flex: 1, minHeight: 0, overflowY: 'auto', padding: '20px 28px 32px' },
  count: {
    display: 'inline-flex', alignItems: 'center', gap: 6, flexShrink: 0,
    color: 'var(--text-muted)', fontSize: 13, fontWeight: 600, fontVariantNumeric: 'tabular-nums',
  },
  searchBox: {
    display: 'flex', alignItems: 'center', gap: 8, flex: '0 1 260px', minWidth: 160,
    background: 'color-mix(in srgb, var(--surface) 78%, var(--bg))',
    border: '1px solid color-mix(in srgb, var(--border) 76%, transparent)',
    borderRadius: 10, padding: '8px 12px', color: 'var(--text-muted)',
  },
  searchInput: { background: 'none', border: 'none', outline: 'none', color: 'var(--text)', fontSize: 13, width: '100%', fontFamily: 'inherit' },
  newBtn: { ...hybridControlStyles.iconButton, width: 34, minWidth: 34, height: 34, background: 'var(--accent)', color: 'var(--on-accent)' },
  importBtn: { ...hybridControlStyles.iconButton, width: 34, minWidth: 34, height: 34, border: '1px solid var(--accent)' },
  empty: { padding: '28px 16px', fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.7, textAlign: 'center' },
  grid: { display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(180px, 1fr))', gap: 18 },
  card: { position: 'relative', minWidth: 0 },
  cardBtn: {
    display: 'flex', flexDirection: 'column', width: '100%', minWidth: 0,
    background: 'var(--browse-card-background, var(--surface))',
    border: '1px solid var(--browse-card-border-color, var(--border))',
    borderRadius: 'var(--browse-card-radius, 10px)',
    padding: 10, cursor: 'pointer', color: 'var(--text)', textAlign: 'left', fontFamily: 'inherit',
  },
  art: { display: 'block', width: '100%' },
  cardTitle: { marginTop: 8, fontSize: 14, fontWeight: 700, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  cardMeta: { marginTop: 2, fontSize: 12, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  smartBadge: {
    position: 'absolute', top: 8, left: 8, width: 28, height: 28, borderRadius: 8, color: 'var(--accent)',
    background: 'color-mix(in srgb, black 55%, transparent)', display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
  },
  cardKebabWrap: {
    position: 'absolute', top: 16, right: 16, borderRadius: 8,
    background: 'color-mix(in srgb, black 45%, transparent)',
  },
};

// ─── Top-level PlaylistsView ──────────────────────────────────────────────────

interface Props {
  playTrack: (track: Track, all?: Track[], source?: import('../types').QueueSource) => void;
  addToQueue: (track: Track) => void;
  initialPlaylistId?: EntityId | null;
  onOpenAlbum?: (album: import('../types').Album) => void;
  onOpenArtist?: (artist: import('../types').Artist) => void;
  /** Deep-links to the Mixes library, prefilling its search with `playlistName`. */
  onOpenMixes?: (playlistName: string) => void;
  /** Bumped (e.g. `Date.now()`) to return to the playlist list from a drilled-in
   * playlist — same "sidebar nav item while already on this view" behavior as
   * BrowseView's `resetRequest`. */
  resetRequest?: number | null;
}

/** Playlists View is part of this module's public API. */
export default function PlaylistsView({
  playTrack, addToQueue, initialPlaylistId,
  onOpenAlbum = () => {}, onOpenArtist = () => {}, onOpenMixes = () => {},
  resetRequest,
}: Props) {
  const [playlists, setPlaylists]   = useState<Playlist[]>([]);
  const [selected, setSelected]     = useState<Playlist | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [createError, setCreateError] = useState('');
  const [optionsPlaylist, setOptionsPlaylist] = useState<Playlist | null>(null);
  const [importFile, setImportFile] = useState<File | null>(null);
  // The rule editor: `null` playlist creates a smart playlist.
  const [smartEditor, setSmartEditor] = useState<{ playlist: Playlist | null } | null>(null);
  const [saveCopyOf, setSaveCopyOf] = useState<Playlist | null>(null);
  const [detailReload, setDetailReload] = useState(0);
  const [imported, setImported] = useState<ImportedPlaylist | null>(null);
  const dismissImported = useCallback(() => setImported(null), []);

  const loadPlaylists = useCallback(async () => {
    const list = await api.playlists.list();
    setPlaylists(list);
    setSelected(current => current ? list.find(p => p.id === current.id) ?? null : null);
    setOptionsPlaylist(current => current ? list.find(p => p.id === current.id) ?? null : null);
  }, []);

  useEffect(() => { void loadPlaylists(); }, [loadPlaylists]);

  // Apply an external navigation request once, not on every list refresh.
  const appliedInitialId = useRef<EntityId | null>(null);
  useEffect(() => {
    if (!initialPlaylistId) { appliedInitialId.current = null; return; }
    if (appliedInitialId.current === initialPlaylistId) return;
    const pl = playlists.find(p => p.id === initialPlaylistId);
    if (pl) { appliedInitialId.current = initialPlaylistId; setSelected(pl); }
  }, [initialPlaylistId, playlists]);

  // Allow external navigation (sidebar "Playlists" click while drilled into a
  // playlist) to return to the list — mirrors BrowseView's resetRequest.
  // Seeded with the mount-time value so a stale token left over from an
  // earlier sidebar click doesn't fire on remount and clobber an
  // initialPlaylistId deep-link navigating into a specific playlist in this
  // same mount.
  const seenResetRequestRef = useRef(resetRequest);
  useEffect(() => {
    if (resetRequest == null) return;
    if (seenResetRequestRef.current === resetRequest) return;
    seenResetRequestRef.current = resetRequest;
    setSelected(null);
  }, [resetRequest]);

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

  const handleImported = async (result: ImportedPlaylist) => {
    setImportFile(null);
    setImported(result);
    await loadPlaylists();
    setSelected(result.playlist);
  };

  const handleSmartSaved = async (saved: Playlist) => {
    const created = !smartEditor?.playlist;
    setSmartEditor(null);
    await loadPlaylists();
    if (created) setSelected(saved);
    else setDetailReload(n => n + 1);
  };

  const handleCopySaved = async (copy: Playlist) => {
    setSaveCopyOf(null);
    await loadPlaylists();
    setSelected(copy);
  };

  const handleDeleted = async (id: EntityId) => {
    setOptionsPlaylist(null);
    setSelected(current => current?.id === id ? null : current);
    await loadPlaylists();
  };

  return (
    <div
      data-ui-design="hybrid"
      style={{ flex: 1, display: 'flex', minHeight: 0, overflow: 'hidden', ...hybridPlaylistStyles.root }}
    >
      {!selected && (
        <PlaylistsGrid
          playlists={playlists}
          onOpen={pl => setSelected(pl)}
          onOptions={setOptionsPlaylist}
          onCreate={() => { setCreateError(''); setShowCreate(true); }}
          onCreateSmart={() => setSmartEditor({ playlist: null })}
          onImport={setImportFile}
        />
      )}
      {selected && (
        <PlaylistDetail
          key={`detail-${selected.id}`}
          playlist={selected}
          onUpdate={loadPlaylists}
          onOptions={() => setOptionsPlaylist(selected)}
          onBack={() => setSelected(null)}
          playTrack={playTrack}
          addToQueue={addToQueue}
          onOpenAlbum={onOpenAlbum}
          onOpenArtist={onOpenArtist}
          onOpenMixes={onOpenMixes}
          onEditRules={() => setSmartEditor({ playlist: selected })}
          reloadToken={detailReload}
        />
      )}

      {/* Detail and Options are siblings for the same playlist, so their keys need distinct prefixes;
          a shared key remounted the detail on every App render and left the old copy in the DOM. */}
      {optionsPlaylist && (
        <PlaylistOptions key={`options-${optionsPlaylist.id}`} playlist={optionsPlaylist} onUpdate={loadPlaylists}
          onDelete={handleDeleted} onClose={() => setOptionsPlaylist(null)}
          onEditRules={() => { setOptionsPlaylist(null); setSmartEditor({ playlist: optionsPlaylist }); }}
          onSaveCopy={() => { setOptionsPlaylist(null); setSaveCopyOf(optionsPlaylist); }} />
      )}
      {smartEditor && (
        <SmartPlaylistEditor
          key={`smart-${smartEditor.playlist?.id ?? 'new'}`}
          playlist={smartEditor.playlist}
          existingNames={playlists.map(p => p.name)}
          onClose={() => setSmartEditor(null)}
          onSaved={saved => void handleSmartSaved(saved)}
        />
      )}
      {saveCopyOf && (
        <SaveAsOrdinaryDialog
          playlist={saveCopyOf}
          existingNames={playlists.map(p => p.name)}
          onCancel={() => setSaveCopyOf(null)}
          onSaved={copy => void handleCopySaved(copy)}
        />
      )}
      {showCreate && (
        <PlaylistDialog
          onSave={handleCreate}
          onCancel={() => { setCreateError(''); setShowCreate(false); }}
          error={createError}
        />
      )}
      {importFile && (
        <PlaylistImportDialog file={importFile} onClose={() => setImportFile(null)} onImported={(result) => void handleImported(result)} />
      )}
      {imported && <PlaylistImportToast added={imported.added} leftOut={imported.leftOut} onDismiss={dismissImported} />}
    </div>
  );
}

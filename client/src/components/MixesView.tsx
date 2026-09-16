/**
 * Defines the Mixes library view: every rendered BoogieMix output for the
 * signed-in user, independent of the playlist that spawned it.
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { api } from '../api';
import type { BoogieMixOutput, ClientEntityId, Track } from '../types';
import { PlaylistArtwork, mixOutputToTrack } from './PlaylistsView';
import MixStoryView from './MixStoryView';
import type { PlaybackSnapshot } from './Player';

// ─── Icons ────────────────────────────────────────────────────────────────────

const SearchIcon = () => <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="11" cy="11" r="7"/><path d="M21 21l-4.3-4.3"/></svg>;
const SortIcon = () => <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M3 7h11M3 12h7M3 17h4"/><path d="M17 4v16M17 4l-3 3M17 4l3 3"/></svg>;
const GridViewIcon = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/></svg>;
const TableViewIcon = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="4" width="18" height="16" rx="1"/><path d="M3 10h18M9 4v16"/></svg>;
const PlayIcon = ({ size = 13 }: { size?: number }) => <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>;
const DownloadIcon = () => <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><path d="M7 10l5 5 5-5"/><path d="M12 15V3"/></svg>;
const EditIcon = ({ size = 13 }: { size?: number }) => <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 013 3L7 19l-4 1 1-4z"/></svg>;
const TrashIcon = () => <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M3 6h18"/><path d="M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2m3 0-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6"/></svg>;
const ClockIcon = () => <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 3"/></svg>;
const SizeIcon = () => <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><path d="M7 10l5 5 5-5"/><path d="M12 15V3"/></svg>;
const CalendarIcon = () => <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="4" width="18" height="18" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/></svg>;
const PlaylistIcon = () => <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>;
const XIcon = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>;
const AlertTriangleIcon = () => <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><path d="M12 9v4M12 17h.01"/></svg>;
const EmptyIcon = () => <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><rect x="3" y="4" width="18" height="4" rx="1"/><rect x="3" y="10" width="12" height="4" rx="1"/><rect x="3" y="16" width="15" height="4" rx="1"/></svg>;
const StoryIcon = () => <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M3 12h4l3-9 4 18 3-9h4"/></svg>;

// ─── Helpers ──────────────────────────────────────────────────────────────────

type SortMode = 'newest' | 'oldest' | 'name' | 'longest' | 'largest';
type ViewMode = 'grid' | 'table';

const VIEW_MODE_STORAGE_KEY = 'boogiebox.mixes.viewMode.v1';

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

function formatDuration(sec: number | null): string {
  if (!sec || sec <= 0) return '–';
  const total = Math.round(sec);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}

function formatSize(bytes: number | null): string {
  if (!bytes || bytes <= 0) return '–';
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`;
  return `${Math.round(mb)} MB`;
}

function formatDate(raw: string): string {
  const d = new Date(raw.includes('T') || raw.includes('Z') ? raw : `${raw.replace(' ', 'T')}Z`);
  if (Number.isNaN(d.getTime())) return '';
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}

function parseCoverAlbumIds(raw: string | null): ClientEntityId[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) return parsed as ClientEntityId[];
  } catch {}
  return [];
}

function sortOutputs(outputs: BoogieMixOutput[], mode: SortMode): BoogieMixOutput[] {
  const copy = [...outputs];
  switch (mode) {
    case 'oldest':
      return copy.sort((a, b) => (a.created_at || '').localeCompare(b.created_at || ''));
    case 'name':
      return copy.sort((a, b) => a.name.localeCompare(b.name));
    case 'longest':
      return copy.sort((a, b) => (b.duration_sec ?? 0) - (a.duration_sec ?? 0));
    case 'largest':
      return copy.sort((a, b) => (b.file_size_bytes ?? 0) - (a.file_size_bytes ?? 0));
    case 'newest':
    default:
      return copy.sort((a, b) => (b.created_at || '').localeCompare(a.created_at || ''));
  }
}

// ─── Delete confirm dialog ────────────────────────────────────────────────────

function DeleteMixDialog({
  output, onConfirm, onCancel,
}: {
  output: BoogieMixOutput;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const trigger = document.activeElement as HTMLElement | null;
    const dialog = ref.current;
    dialog?.showModal();
    return () => { dialog?.close(); if (trigger?.isConnected) trigger.focus(); };
  }, []);
  return createPortal(
    <dialog
      ref={ref}
      aria-label="Delete this mix?"
      onCancel={e => { e.preventDefault(); onCancel(); }}
      onClick={e => { if (e.target === e.currentTarget) onCancel(); }}
      style={S.dialogOverlay}
    >
      <div style={S.dialogBox}>
        <div style={S.dialogIconWrap}><AlertTriangleIcon /></div>
        <h3 style={S.dialogTitle}>Delete this mix?</h3>
        <p style={S.dialogText}>
          Delete <b style={{ color: 'var(--text)' }}>&ldquo;{output.name}&rdquo;</b>? This removes the rendered file.
          {output.playlist_name ? <> The source playlist <b style={{ color: 'var(--text)' }}>{output.playlist_name}</b> is not affected.</> : null}
          {' '}This can&rsquo;t be undone.
        </p>
        <div style={{ display: 'flex', gap: 8 }}>
          <button type="button" style={S.btnSecondary} onClick={onCancel}>Cancel</button>
          <button type="button" style={S.btnDanger} onClick={onConfirm}>
            <TrashIcon /> Delete
          </button>
        </div>
      </div>
    </dialog>,
    document.body,
  );
}

// ─── Editable name ────────────────────────────────────────────────────────────

function EditableName({
  name, onRename, textStyle,
}: {
  name: string;
  onRename: (name: string) => void;
  textStyle: React.CSSProperties;
}) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(name);
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => { if (editing) inputRef.current?.focus(); }, [editing]);
  useEffect(() => { setValue(name); }, [name]);

  const commit = () => {
    const trimmed = value.trim();
    setEditing(false);
    if (trimmed && trimmed !== name) onRename(trimmed);
    else setValue(name);
  };

  if (editing) {
    return (
      <input
        ref={inputRef}
        value={value}
        onChange={e => setValue(e.target.value)}
        onBlur={commit}
        onClick={e => e.stopPropagation()}
        onKeyDown={e => {
          if (e.key === 'Enter') commit();
          if (e.key === 'Escape') { setValue(name); setEditing(false); }
        }}
        style={{ ...textStyle, background: 'var(--surface-subtle)', border: '1px solid var(--accent)', borderRadius: 6, padding: '2px 6px', minWidth: 0 }}
      />
    );
  }
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, minWidth: 0 }}>
      <span style={textStyle}>{name}</span>
      <button
        type="button"
        aria-label={`Rename ${name}`}
        title="Rename"
        style={S.editIconBtn}
        onClick={(e) => { e.stopPropagation(); setEditing(true); }}
      >
        <EditIcon />
      </button>
    </span>
  );
}

// ─── Top-level MixesView ──────────────────────────────────────────────────────

interface Props {
  playTrack: (track: Track, all?: Track[], source?: import('../types').QueueSource) => void;
  playbackSnapshot?: PlaybackSnapshot | null;
  openRequest?: { playlistName: string; token: number } | null;
  onOpenPlaylist: (playlistId: ClientEntityId) => void;
}

/** Mixes View is part of this module's public API. */
export default function MixesView({ playTrack, playbackSnapshot, openRequest, onOpenPlaylist }: Props) {
  const [outputs, setOutputs] = useState<BoogieMixOutput[]>([]);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState('');
  const [sortMode, setSortMode] = useState<SortMode>('newest');
  const [viewMode, setViewMode] = useState<ViewMode>(() => (safeLocalStorageGet(VIEW_MODE_STORAGE_KEY) as ViewMode) || 'grid');
  const [deleteTarget, setDeleteTarget] = useState<BoogieMixOutput | null>(null);
  const [storyOutput, setStoryOutput] = useState<BoogieMixOutput | null>(null);
  const appliedRequestToken = useRef<number | null>(null);

  const load = useCallback(async () => {
    if (!api.boogiemix?.listAllOutputs) { setOutputs([]); setLoading(false); return; }
    setLoading(true);
    try {
      const list = await api.boogiemix.listAllOutputs();
      setOutputs(list);
    } catch {
      setOutputs([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  useEffect(() => {
    if (!openRequest || appliedRequestToken.current === openRequest.token) return;
    appliedRequestToken.current = openRequest.token;
    setQuery(openRequest.playlistName);
  }, [openRequest]);

  const setPersistedViewMode = useCallback((mode: ViewMode) => {
    safeLocalStorageSet(VIEW_MODE_STORAGE_KEY, mode);
    setViewMode(mode);
  }, []);

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const base = needle
      ? outputs.filter(o =>
          o.name.toLowerCase().includes(needle) ||
          (o.playlist_name ?? '').toLowerCase().includes(needle))
      : outputs;
    return sortOutputs(base, sortMode);
  }, [outputs, query, sortMode]);

  const handlePlay = (output: BoogieMixOutput) => {
    const mixTrack = mixOutputToTrack(output, output.playlist_name || output.name);
    playTrack(mixTrack, [mixTrack]);
  };

  const handleRename = async (output: BoogieMixOutput, name: string) => {
    setOutputs(prev => prev.map(o => (o.id === output.id ? { ...o, name } : o)));
    try {
      if (api.boogiemix?.renameOutput) await api.boogiemix.renameOutput(output.id, name);
    } catch {
      void load();
    }
  };

  const handleDeleteConfirmed = async () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setDeleteTarget(null);
    setOutputs(prev => prev.filter(o => o.id !== target.id));
    setStoryOutput(prev => (prev?.id === target.id ? null : prev));
    try {
      if (api.boogiemix?.deleteOutput) await api.boogiemix.deleteOutput(target.id);
    } catch {
      void load();
    }
  };

  if (storyOutput) {
    return (
      <>
        <MixStoryView
          output={storyOutput}
          playTrack={playTrack}
          playbackSnapshot={playbackSnapshot}
          onBack={() => setStoryOutput(null)}
          onDelete={setDeleteTarget}
        />
        {deleteTarget && (
          <DeleteMixDialog
            output={deleteTarget}
            onConfirm={handleDeleteConfirmed}
            onCancel={() => setDeleteTarget(null)}
          />
        )}
      </>
    );
  }

  return (
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0, overflow: 'hidden', padding: '28px 32px' }}>
      <h1 style={S.h1}>Mixes</h1>
      <div style={S.sub}>Manage the BoogieMix collection</div>

      <div style={S.toolbarRow}>
        <div style={S.search}>
          <SearchIcon />
          <input
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder="Search mixes or playlists…"
            style={S.searchInput}
          />
          {query && (
            <button type="button" aria-label="Clear search" style={S.clearBtn} onClick={() => setQuery('')}><XIcon /></button>
          )}
        </div>
        <label style={S.sortSelect}>
          <SortIcon />
          <select
            value={sortMode}
            onChange={e => setSortMode(e.target.value as SortMode)}
            style={S.sortSelectInput}
            aria-label="Sort mixes"
          >
            <option value="newest">Newest first</option>
            <option value="oldest">Oldest first</option>
            <option value="name">Name A–Z</option>
            <option value="longest">Longest</option>
            <option value="largest">Largest</option>
          </select>
        </label>
        <button
          type="button"
          style={{ ...S.viewToggle, ...(viewMode === 'grid' ? S.viewToggleActive : {}) }}
          onClick={() => setPersistedViewMode('grid')}
          title="Grid view"
          aria-label="Switch to grid view"
          aria-pressed={viewMode === 'grid'}
        >
          <GridViewIcon />
        </button>
        <button
          type="button"
          style={{ ...S.viewToggle, ...(viewMode === 'table' ? S.viewToggleActive : {}) }}
          onClick={() => setPersistedViewMode('table')}
          title="Table view"
          aria-label="Switch to table view"
          aria-pressed={viewMode === 'table'}
        >
          <TableViewIcon />
        </button>
      </div>

      <div style={{ flex: 1, overflowY: 'auto' }}>
        {loading && <div style={S.empty}>Loading…</div>}
        {!loading && filtered.length === 0 && (
          <div style={S.empty}>
            <EmptyIcon />
            <div style={{ marginTop: 12, fontSize: 16, fontWeight: 500, color: 'var(--text)' }}>
              {outputs.length === 0 ? 'No mixes yet' : 'No mixes match your search'}
            </div>
            <div style={{ marginTop: 6, fontSize: 14, color: 'var(--text-muted)' }}>
              {outputs.length === 0 ? 'Start a BoogieMix from any playlist to see it here.' : 'Try a different search term.'}
            </div>
          </div>
        )}
        {!loading && filtered.length > 0 && viewMode === 'grid' && (
          <div style={S.grid}>
            {filtered.map(output => (
              <MixCard
                key={output.id}
                output={output}
                onPlay={() => handlePlay(output)}
                onRename={name => handleRename(output, name)}
                onDelete={() => setDeleteTarget(output)}
                onOpenPlaylist={onOpenPlaylist}
                onOpenStory={() => setStoryOutput(output)}
              />
            ))}
          </div>
        )}
        {!loading && filtered.length > 0 && viewMode === 'table' && (
          <MixTable
            outputs={filtered}
            onPlay={handlePlay}
            onRename={handleRename}
            onDelete={setDeleteTarget}
            onOpenPlaylist={onOpenPlaylist}
            onOpenStory={setStoryOutput}
          />
        )}
      </div>

      {deleteTarget && (
        <DeleteMixDialog
          output={deleteTarget}
          onConfirm={handleDeleteConfirmed}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </div>
  );
}

// ─── Grid card ────────────────────────────────────────────────────────────────

function MixCard({
  output, onPlay, onRename, onDelete, onOpenPlaylist, onOpenStory,
}: {
  output: BoogieMixOutput;
  onPlay: () => void;
  onRename: (name: string) => void;
  onDelete: () => void;
  onOpenPlaylist: (playlistId: ClientEntityId) => void;
  onOpenStory: () => void;
}) {
  const albumIds = parseCoverAlbumIds(output.cover_album_ids);
  return (
    <div style={S.card}>
      <div style={{ ...S.cover, cursor: 'pointer' }} onClick={onOpenStory} title={`View breakdown: ${output.name}`}>
        <PlaylistArtwork albumIds={albumIds} responsive />
        <button type="button" aria-label={`Play ${output.name}`} title="Play" style={S.coverPlayBtn} onClick={(e) => { e.stopPropagation(); onPlay(); }}>
          <PlayIcon size={12} />
        </button>
      </div>
      <div style={S.cardName}>
        <EditableName name={output.name} onRename={onRename} textStyle={S.cardNameText} />
      </div>
      {output.playlist_name && (
        output.playlist_id ? (
          <button type="button" style={S.playlistChipBtn} onClick={() => onOpenPlaylist(output.playlist_id!)} title={`Go to playlist: ${output.playlist_name}`}>
            <PlaylistIcon /> {output.playlist_name}
          </button>
        ) : (
          <span style={S.playlistChip}><PlaylistIcon /> {output.playlist_name}</span>
        )
      )}
      <div style={S.statRow}>
        <span><ClockIcon /> {formatDuration(output.duration_sec)}</span>
        <span><SizeIcon /> {formatSize(output.file_size_bytes)}</span>
        <span><CalendarIcon /> {formatDate(output.created_at)}</span>
      </div>
      <div style={S.cardActions}>
        <button type="button" style={S.iconBtnPrimary} title="Play" aria-label={`Play ${output.name}`} onClick={onPlay}><PlayIcon /></button>
        <button type="button" style={S.iconBtn} title="View breakdown" aria-label={`View breakdown of ${output.name}`} onClick={onOpenStory}><StoryIcon /></button>
        <a
          style={S.iconBtn}
          title="Download"
          aria-label={`Download ${output.name}`}
          href={api.boogiemix ? api.boogiemix.outputDownloadUrl(output.id) : '#'}
        >
          <DownloadIcon />
        </a>
        <button type="button" style={{ ...S.iconBtn, ...S.iconBtnDanger }} title="Delete" aria-label={`Delete ${output.name}`} onClick={onDelete}><TrashIcon /></button>
      </div>
    </div>
  );
}

// ─── Table ────────────────────────────────────────────────────────────────────

function MixTable({
  outputs, onPlay, onRename, onDelete, onOpenPlaylist, onOpenStory,
}: {
  outputs: BoogieMixOutput[];
  onPlay: (output: BoogieMixOutput) => void;
  onRename: (output: BoogieMixOutput, name: string) => void;
  onDelete: (output: BoogieMixOutput) => void;
  onOpenPlaylist: (playlistId: ClientEntityId) => void;
  onOpenStory: (output: BoogieMixOutput) => void;
}) {
  return (
    <table style={S.table}>
      <thead>
        <tr>
          <th style={S.th}>Name</th>
          <th style={S.th}>Playlist</th>
          <th style={S.th}>Duration</th>
          <th style={S.th}>Size</th>
          <th style={S.th}>Created</th>
          <th style={S.th} />
        </tr>
      </thead>
      <tbody>
        {outputs.map(output => (
          <tr key={output.id}>
            <td style={S.td}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0, cursor: 'pointer' }} onClick={() => onOpenStory(output)} title={`View breakdown: ${output.name}`}>
                <span style={S.miniIcon}><PlaylistArtwork albumIds={parseCoverAlbumIds(output.cover_album_ids)} responsive /></span>
                <span onClick={(e) => e.stopPropagation()} style={{ minWidth: 0 }}>
                  <EditableName name={output.name} onRename={name => onRename(output, name)} textStyle={S.rowNameText} />
                </span>
              </div>
            </td>
            <td style={{ ...S.td, ...S.mutedCell }}>
              {output.playlist_name ? (
                output.playlist_id ? (
                  <button type="button" style={S.playlistLinkBtn} onClick={() => onOpenPlaylist(output.playlist_id!)}>{output.playlist_name}</button>
                ) : output.playlist_name
              ) : '–'}
            </td>
            <td style={{ ...S.td, ...S.mutedCell }}>{formatDuration(output.duration_sec)}</td>
            <td style={{ ...S.td, ...S.mutedCell }}>{formatSize(output.file_size_bytes)}</td>
            <td style={{ ...S.td, ...S.mutedCell }}>{formatDate(output.created_at)}</td>
            <td style={S.td}>
              <div style={S.rowActions}>
                <button type="button" style={S.iconBtnPrimary} title="Play" aria-label={`Play ${output.name}`} onClick={() => onPlay(output)}><PlayIcon size={12} /></button>
                <button type="button" style={S.iconBtn} title="View breakdown" aria-label={`View breakdown of ${output.name}`} onClick={() => onOpenStory(output)}><StoryIcon /></button>
                <a
                  style={S.iconBtn}
                  title="Download"
                  aria-label={`Download ${output.name}`}
                  href={api.boogiemix ? api.boogiemix.outputDownloadUrl(output.id) : '#'}
                >
                  <DownloadIcon />
                </a>
                <button type="button" style={{ ...S.iconBtn, ...S.iconBtnDanger }} title="Delete" aria-label={`Delete ${output.name}`} onClick={() => onDelete(output)}><TrashIcon /></button>
              </div>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

// ─── Styles ───────────────────────────────────────────────────────────────────

const S: Record<string, React.CSSProperties> = {
  h1: { fontSize: 20, margin: '0 0 4px', color: 'var(--text)' },
  sub: { fontSize: 13, color: 'var(--text-muted)', marginBottom: 18 },
  toolbarRow: { display: 'flex', alignItems: 'center', gap: 10, marginBottom: 18, flexWrap: 'wrap' },
  search: { display: 'flex', alignItems: 'center', gap: 6, background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: '6px 10px', color: 'var(--text-muted)', fontSize: 13, width: 240 },
  searchInput: { flex: 1, background: 'transparent', border: 'none', outline: 'none', color: 'var(--text)', fontSize: 13, fontFamily: 'inherit' },
  clearBtn: { background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', display: 'flex', alignItems: 'center', padding: 0 },
  sortSelect: { display: 'flex', alignItems: 'center', gap: 6, background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: '6px 10px', color: 'var(--text)', fontSize: 13, marginLeft: 'auto' },
  sortSelectInput: { background: 'transparent', border: 'none', outline: 'none', color: 'var(--text)', fontSize: 13, fontFamily: 'inherit' },
  viewToggle: { width: 30, height: 30, borderRadius: 8, border: '1px solid var(--border)', background: 'var(--surface)', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)', cursor: 'pointer' },
  viewToggleActive: { borderColor: 'var(--accent)', color: 'var(--accent)', background: 'color-mix(in srgb, var(--accent) 12%, transparent)' },
  empty: { padding: '48px 0', textAlign: 'center', color: 'var(--text-muted)', display: 'flex', flexDirection: 'column', alignItems: 'center' },
  grid: { display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(230px, 1fr))', gap: 16, paddingBottom: 24 },
  card: { background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 10, padding: 14, display: 'flex', flexDirection: 'column', gap: 10 },
  cover: { position: 'relative', borderRadius: 8, overflow: 'hidden' },
  coverPlayBtn: { position: 'absolute', bottom: 8, right: 8, width: 30, height: 30, borderRadius: '50%', background: 'var(--accent)', border: 'none', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'white', cursor: 'pointer', boxShadow: '0 2px 8px rgba(0,0,0,.4)' },
  cardName: { display: 'flex', alignItems: 'center', minWidth: 0 },
  cardNameText: { fontSize: 14, fontWeight: 500, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  playlistChip: { display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 11, color: 'var(--text-muted)', background: 'var(--surface-subtle)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px', width: 'fit-content' },
  playlistChipBtn: { display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 11, color: 'var(--accent)', background: 'var(--surface-subtle)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px', width: 'fit-content', cursor: 'pointer', fontFamily: 'inherit' },
  playlistLinkBtn: { background: 'none', border: 'none', padding: 0, color: 'var(--accent)', cursor: 'pointer', font: 'inherit' },
  statRow: { display: 'flex', gap: 14, fontSize: 11, color: 'var(--text-muted)' },
  cardActions: { display: 'flex', gap: 6, marginTop: 2 },
  iconBtn: { width: 28, height: 28, borderRadius: 6, border: '1px solid var(--border)', background: 'var(--surface-subtle)', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)', cursor: 'pointer', textDecoration: 'none' },
  iconBtnPrimary: { width: 28, height: 28, borderRadius: 6, border: '1px solid var(--accent)', background: 'var(--accent)', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'white', cursor: 'pointer' },
  iconBtnDanger: { color: 'var(--danger)' },
  editIconBtn: { background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', display: 'flex', alignItems: 'center', padding: 0, flexShrink: 0 },
  table: { width: '100%', borderCollapse: 'collapse', fontSize: 13 },
  th: { textAlign: 'left', fontSize: 11, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '.04em', padding: '8px 12px', borderBottom: '1px solid var(--border)', fontWeight: 500 },
  td: { padding: '10px 12px', borderBottom: '1px solid var(--border)', verticalAlign: 'middle' },
  mutedCell: { color: 'var(--text-muted)' },
  miniIcon: { width: 26, height: 26, borderRadius: 6, overflow: 'hidden', flexShrink: 0, display: 'block' },
  rowNameText: { fontSize: 13, fontWeight: 500, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  rowActions: { display: 'flex', gap: 6, justifyContent: 'flex-end' },
  dialogOverlay: { position: 'fixed', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 200, background: 'rgba(0,0,0,.55)', border: 0, padding: 0, margin: 0, width: '100%', height: '100%', maxWidth: '100%', maxHeight: '100%' },
  dialogBox: { width: 'min(380px, calc(100% - 32px))', background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 12, padding: 22, boxShadow: '0 20px 60px rgba(0,0,0,.5)' },
  dialogIconWrap: { width: 40, height: 40, borderRadius: 10, background: 'color-mix(in srgb, var(--danger) 16%, transparent)', color: 'var(--danger)', display: 'flex', alignItems: 'center', justifyContent: 'center', marginBottom: 14 },
  dialogTitle: { fontSize: 15, margin: '0 0 8px', color: 'var(--text)' },
  dialogText: { fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6, margin: '0 0 20px' },
  btnSecondary: { flex: 1, padding: 10, borderRadius: 8, border: '1px solid var(--border)', background: 'var(--surface-subtle)', color: 'var(--text)', fontSize: 13, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 7, cursor: 'pointer', fontFamily: 'inherit' },
  btnDanger: { flex: 1, padding: 10, borderRadius: 8, border: '1px solid var(--danger)', background: 'var(--danger)', color: 'white', fontSize: 13, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 7, cursor: 'pointer', fontFamily: 'inherit' },
};

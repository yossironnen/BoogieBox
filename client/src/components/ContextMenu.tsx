/**
 * Defines the Context Menu React component and related UI helpers.
 */

import React, { useState, useEffect, useLayoutEffect, useRef, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { api } from '../api';
import type { ClientEntityId, CrossfadeMode } from '../types';
import type { EntityId } from '../entityId';

// ─── Types ────────────────────────────────────────────────────────────────────

/** Menu Position is part of this module's public API. */
export interface MenuPosition { x: number; y: number }

/** Context Target is part of this module's public API. */
export type ContextTarget =
  | { kind: 'track'; trackId: ClientEntityId; title: string }
  | { kind: 'album'; albumId: ClientEntityId; title: string }
  | { kind: 'artist'; artistId: ClientEntityId; name: string }
  | { kind: 'library'; libraryId: ClientEntityId; name: string }
  | { kind: 'playlist'; playlistId: EntityId; name: string };

/** Context Action Icon is part of this module's public API. */
export type ContextActionIcon = 'play' | 'scan' | 'cancel' | 'deep-analysis';

/** Context Menu Action is part of this module's public API. */
export interface ContextMenuAction {
  id: string;
  label: string;
  icon: ContextActionIcon;
  disabled?: boolean;
  dividerBefore?: boolean;
  onSelect: () => void | Promise<void>;
}

/** Context Callbacks is part of this module's public API. */
export interface ContextCallbacks {
  onPlay?: () => void;
  onQueue?: () => void;
  onOpen?: () => void;
  onRename?: () => void;
  onDelete?: () => void;
  onRemove?: () => void;
  actions?: ContextMenuAction[];
}

// ─── Global context menu state (singleton) ───────────────────────────────────
// One menu open at a time, managed via a global event so any component
// can trigger it without prop-drilling.

const CTX_EVENT = 'boogiebox:contextmenu';

/** Fired when a track's "Info" action is chosen; App.tsx owns the popup that listens for it. */
export const TRACK_INFO_EVENT = 'boogiebox:trackinfo';

/** Opens the Track Info popup for the given track (used by the kebab menu's "Info" entry). */
export function openTrackInfo(trackId: ClientEntityId) {
  window.dispatchEvent(new CustomEvent(TRACK_INFO_EVENT, { detail: { trackId } }));
}

/** Gap kept between the menu and every viewport edge. */
const MENU_MARGIN = 8;
/** Floor for the height cap so the menu stays usable on very short viewports. */
const MENU_MIN_HEIGHT = 120;

function normalizePlaylistName(name: string): string {
  return name.trim().replace(/\s+/g, ' ').toLowerCase();
}

function targetDisplayName(target: ContextTarget): string {
  if (target.kind === 'artist') return target.name;
  if (target.kind === 'library') return target.name;
  if (target.kind === 'playlist') return target.name;
  return target.title;
}

function targetKindLabel(target: ContextTarget): string {
  if (target.kind === 'track') return 'Track';
  if (target.kind === 'album') return 'Album';
  if (target.kind === 'artist') return 'Artist';
  if (target.kind === 'library') return 'Library';
  return 'Playlist';
}

/** Legacy right-click trigger (kept for backwards compat during migration). */
export function openContextMenu(
  e: React.MouseEvent,
  target: ContextTarget,
  callbacks: ContextCallbacks,
) {
  e.preventDefault();
  e.stopPropagation();
  window.dispatchEvent(new CustomEvent(CTX_EVENT, {
    // flipY: bottom edge to align against when the menu has to open upwards.
    detail: { x: e.clientX, y: e.clientY, flipY: e.clientY, target, callbacks },
  }));
}

/** Left-click kebab trigger — anchors the menu below the button rect. */
export function openKebabMenu(
  rect: DOMRect,
  target: ContextTarget,
  callbacks: ContextCallbacks,
  trigger?: HTMLElement,
) {
  const menuW = 220;
  const x = Math.max(8, rect.right - menuW);
  const y = rect.bottom + 4;
  window.dispatchEvent(new CustomEvent(CTX_EVENT, {
    // flipY sits above the button so an upward menu does not cover its trigger.
    detail: { x, y, flipY: rect.top - 4, target, callbacks, trigger },
  }));
}

// ─── Kebab Button ────────────────────────────────────────────────────────────

const KebabSVG = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" style={{ flexShrink: 0 }}>
    <circle cx="12" cy="5" r="2"/>
    <circle cx="12" cy="12" r="2"/>
    <circle cx="12" cy="19" r="2"/>
  </svg>
);

const kebabBase: React.CSSProperties = {
  background: 'transparent',
  border: 'none',
  color: 'var(--text-muted)',
  cursor: 'pointer',
  padding: 4,
  borderRadius: 4,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  flexShrink: 0,
  transition: 'opacity 0.15s',
};

/** Kebab Button is part of this module's public API. */
export function KebabButton({ target, callbacks, visible = true, style }: {
  target: ContextTarget;
  callbacks: ContextCallbacks;
  visible?: boolean;
  style?: React.CSSProperties;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  return (
    <button
      ref={ref}
      aria-haspopup="menu"
      aria-label="More actions"
      style={{ ...kebabBase, opacity: visible ? 1 : 0, pointerEvents: visible ? 'auto' : 'none', ...style }}
      onClick={e => {
        e.stopPropagation();
        e.preventDefault();
        const rect = ref.current!.getBoundingClientRect();
        openKebabMenu(rect, target, callbacks, ref.current!);
      }}
      onMouseDown={e => e.stopPropagation()}
    >
      <KebabSVG />
    </button>
  );
}

// ─── New Playlist inline form ─────────────────────────────────────────────────

function NewPlaylistInline({ onCreated, onCancel, existingNames }: {
  onCreated: (id: EntityId, name: string) => void;
  onCancel: () => void;
  existingNames: string[];
}) {
  const [name, setName] = useState('');
  const [error, setError] = useState('');
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => { ref.current?.focus(); }, []);

  const submit = async () => {
    const n = name.trim();
    if (!n) return;
    const normalized = normalizePlaylistName(n);
    if (existingNames.some((existingName) => normalizePlaylistName(existingName) === normalized)) {
      setError('A playlist with this name already exists');
      return;
    }
    try {
      const pl = await api.playlists.create(n);
      setError('');
      onCreated(pl.id, pl.name);
    } catch (e: any) {
      setError(e?.message || 'Could not create playlist');
    }
  };

  return (
    <div style={CM.newPlRow}>
      <input
        ref={ref}
        style={CM.newPlInput}
        placeholder="Playlist name…"
        value={name}
        onChange={e => { setName(e.target.value); if (error) setError(''); }}
        onKeyDown={e => {
          if (e.key === 'Enter') { e.stopPropagation(); submit(); }
          if (e.key === 'Escape') { e.stopPropagation(); onCancel(); }
        }}
        onClick={e => e.stopPropagation()}
      />
      <button
        style={CM.newPlCreate}
        disabled={!name.trim()}
        onClick={e => { e.stopPropagation(); submit(); }}
      >
        Create
      </button>
      {error && <div style={CM.newPlError}>{error}</div>}
    </div>
  );
}

// ─── Playlist submenu ─────────────────────────────────────────────────────────

function PlaylistSubmenu({ target, onDone }: {
  target: ContextTarget;
  onDone: () => void;
}) {
  const [playlists, setPlaylists] = useState<{ id: EntityId; name: string }[]>([]);
  const [feedback, setFeedback]   = useState<Record<EntityId, 'adding' | 'done'>>({});
  const [showNew, setShowNew]     = useState(false);
  const [loading, setLoading]     = useState(true);

  useEffect(() => {
    api.playlists.list().then(list => { setPlaylists(list); setLoading(false); });
  }, []);

  const addToPlaylist = async (playlistId: EntityId) => {
    if (feedback[playlistId]) return;
    setFeedback(f => ({ ...f, [playlistId]: 'adding' }));
    try {
      if (target.kind === 'track') {
        await api.playlists.addTrack(playlistId, target.trackId);
      } else if (target.kind === 'album') {
        const tracks = await api.albumTracks(target.albumId);
        await api.playlists.addTracks(playlistId, tracks.map(t => t.id));
      }
      setFeedback(f => ({ ...f, [playlistId]: 'done' }));
      setTimeout(onDone, 600);
    } catch {
      setFeedback(f => { const n = { ...f }; delete n[playlistId]; return n; });
    }
  };

  const handleCreated = async (id: EntityId, name: string) => {
    setShowNew(false);
    setPlaylists(prev => [{ id, name }, ...prev]);
    await addToPlaylist(id);
  };

  return (
    <div style={CM.submenu}>
      <div style={CM.submenuHeader}>
        {target.kind === 'track' ? 'Add track to playlist' : 'Add album to playlist'}
      </div>

      {loading && <div style={CM.loadingRow}>Loading…</div>}

      {!loading && playlists.length === 0 && !showNew && (
        <div style={CM.emptyRow}>No playlists yet</div>
      )}

      {playlists.map(pl => {
        const fb = feedback[pl.id];
        return (
          <button
            key={pl.id}
            style={{
              ...CM.plItem,
              color: fb === 'done' ? '#22c55e' : 'var(--text)',
            }}
            onClick={e => { e.stopPropagation(); addToPlaylist(pl.id); }}
          >
            <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {pl.name}
            </span>
            <span style={{ fontSize: 13, color: 'var(--text-muted)', flexShrink: 0 }}>
              {fb === 'adding' ? '…' : fb === 'done' ? '✓' : ''}
            </span>
          </button>
        );
      })}

      {showNew
        ? (
          <NewPlaylistInline
            existingNames={playlists.map((playlist) => playlist.name)}
            onCreated={handleCreated}
            onCancel={() => setShowNew(false)}
          />
        )
        : (
          <button
            style={{ ...CM.plItem, color: 'var(--accent)', borderTop: '1px solid var(--border)', marginTop: 2 }}
            onClick={e => { e.stopPropagation(); setShowNew(true); }}
          >
            + New playlist…
          </button>
        )
      }
    </div>
  );
}

// ─── Crossfade Override Panel ─────────────────────────────────────────────────

function CrossfadeOverridePanel({ entityType, entityId, onDone }: {
  entityType: 'album' | 'playlist';
  entityId: ClientEntityId;
  onDone: () => void;
}) {
  const [mode, setMode] = useState<CrossfadeMode | null>(null);
  const [duration, setDuration] = useState(2);
  const [loading, setLoading] = useState(true);
  const [hasOverride, setHasOverride] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    api.crossfade.config(entityType, entityId).then(cfg => {
      setMode(cfg.mode);
      setDuration(cfg.duration);
      setHasOverride(cfg.source === 'override');
      setLoading(false);
    }).catch(() => { setMode('off'); setLoading(false); });
  }, [entityType, entityId]);

  const save = async (m: CrossfadeMode, d: number) => {
    setSaving(true);
    try {
      await api.crossfade.upsertOverride({ entity_type: entityType, entity_id: entityId, mode: m, duration: d });
      setHasOverride(true);
    } catch { /* ignore */ }
    setSaving(false);
  };

  const reset = async () => {
    setSaving(true);
    try {
      await api.crossfade.removeOverride(entityType, entityId);
      setHasOverride(false);
      const cfg = await api.crossfade.config(entityType, entityId);
      setMode(cfg.mode);
      setDuration(cfg.duration);
    } catch { /* ignore */ }
    setSaving(false);
  };

  if (loading || mode === null) return <div style={CM.loadingRow}>Loading…</div>;

  return (
    <div style={CM.submenu} onClick={e => e.stopPropagation()}>
      <div style={CM.submenuHeader}>Crossfade override</div>

      {/* Mode pills */}
      <div style={{ display: 'flex', gap: 0, margin: '4px 10px 8px', borderRadius: 6, overflow: 'hidden', border: '1px solid var(--border)' }}>
        {([
          { value: 'off' as const, label: 'Off' },
          { value: 'zerogap' as const, label: 'Zero-gap' },
          { value: 'crossfade' as const, label: 'Crossfade' },
        ]).map(opt => (
          <button
            key={opt.value}
            onClick={() => { setMode(opt.value); save(opt.value, duration); }}
            style={{
              flex: 1, padding: '5px 0', fontSize: 13, fontWeight: 600, cursor: 'pointer',
              border: 'none', outline: 'none', fontFamily: 'inherit',
              backgroundColor: mode === opt.value ? 'var(--accent)' : 'var(--bg)',
              color: mode === opt.value ? '#fff' : 'var(--text)',
            }}
          >
            {opt.label}
          </button>
        ))}
      </div>

      {/* Duration slider */}
      {mode === 'crossfade' && (
        <div style={{ padding: '0 10px 8px', display: 'flex', alignItems: 'center', gap: 6 }}>
          <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>1s</span>
          <input
            type="range" min={1} max={10} step={1} value={duration}
            onChange={e => { const v = Number(e.target.value); setDuration(v); save(mode, v); }}
            style={{ flex: 1, accentColor: 'var(--accent)' }}
          />
          <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>10s</span>
          <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', minWidth: 20, textAlign: 'right' }}>{duration}s</span>
        </div>
      )}

      {/* Reset / status */}
      <div style={{ padding: '0 10px 8px', display: 'flex', alignItems: 'center', gap: 8 }}>
        {hasOverride && (
          <button
            onClick={reset}
            style={{
              background: 'transparent', border: '1px solid var(--border)', borderRadius: 5,
              color: 'var(--text-muted)', fontSize: 12, padding: '3px 8px', cursor: 'pointer', fontFamily: 'inherit',
            }}
          >
            Reset to default
          </button>
        )}
        {saving && <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>Saving…</span>}
        {!hasOverride && !saving && <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>Using global default</span>}
      </div>
    </div>
  );
}

// ─── Main Context Menu ────────────────────────────────────────────────────────

/** Context Menu Root is part of this module's public API. */
export function ContextMenuRoot() {
  const [visible, setVisible]     = useState(false);
  const [pos, setPos]             = useState<MenuPosition>({ x: 0, y: 0 });
  const [target, setTarget]       = useState<ContextTarget | null>(null);
  const [callbacks, setCallbacks] = useState<ContextCallbacks | null>(null);
  const [showPlaylists, setShowPlaylists] = useState(false);
  const [showCrossfade, setShowCrossfade] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const visibleRef = useRef(false);
  const activeTriggerRef = useRef<HTMLElement | null>(null);
  const flipYRef = useRef(0);
  /** False until the current menu has been placed, so it is placed exactly once. */
  const placedRef = useRef(false);

  const close = useCallback(() => {
    visibleRef.current = false;
    activeTriggerRef.current = null;
    placedRef.current = false;
    setVisible(false);
    setShowPlaylists(false);
    setShowCrossfade(false);
  }, []);

  // Listen for open events
  useEffect(() => {
    const handler = (e: Event) => {
      const { x, y, flipY, target, callbacks, trigger } = (e as CustomEvent).detail;
      if (trigger && visibleRef.current && activeTriggerRef.current === trigger) {
        close();
        return;
      }
      // Store the requested position as-is; clamping needs the rendered menu
      // height, so it happens in the layout effect below.
      flipYRef.current = typeof flipY === 'number' ? flipY : y;
      // Re-open (possibly from a different anchor) needs a fresh placement.
      placedRef.current = false;
      setPos({ x, y });
      setTarget(target);
      setCallbacks(callbacks);
      setShowPlaylists(false);
      setShowCrossfade(false);
      visibleRef.current = true;
      activeTriggerRef.current = trigger ?? null;
      setVisible(true);
    };
    window.addEventListener(CTX_EVENT, handler);
    return () => window.removeEventListener(CTX_EVENT, handler);
  }, [close]);

  // Place the menu once per open, then leave it alone. Submenus expanding must
  // never slide the menu out from under the pointer, so later growth is
  // absorbed by the height cap (the menu scrolls) instead of by repositioning.
  const positionMenu = useCallback((replace = false) => {
    const el = menuRef.current;
    if (!el) return;
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    if (replace || !placedRef.current) {
      // Measure the natural size, unconstrained by any cap from a previous open.
      el.style.maxHeight = '';
      const { width, height } = el.getBoundingClientRect();

      let left = pos.x;
      if (left + width > vw - MENU_MARGIN) left = vw - MENU_MARGIN - width;
      if (left < MENU_MARGIN) left = MENU_MARGIN;

      let top = pos.y;
      if (top + height > vh - MENU_MARGIN) {
        // Prefer opening upwards from the anchor, but only when the flipped menu
        // fits entirely on screen — the anchor itself can be out of view after a
        // viewport resize. Otherwise pin to the bottom edge.
        const flipped = flipYRef.current - height;
        const flippedFits = flipped >= MENU_MARGIN && flipYRef.current <= vh - MENU_MARGIN;
        top = flippedFits ? flipped : Math.max(MENU_MARGIN, vh - MENU_MARGIN - height);
      }

      el.style.left = `${left}px`;
      el.style.top = `${top}px`;
      placedRef.current = true;
    }

    // Cap to the room left below the settled top edge; taller content scrolls.
    const top = parseFloat(el.style.top) || 0;
    el.style.maxHeight = `${Math.max(MENU_MIN_HEIGHT, vh - MENU_MARGIN - top)}px`;
  }, [pos.x, pos.y]);

  // Runs after every render, but only refreshes the height cap once placed.
  useLayoutEffect(positionMenu);

  useLayoutEffect(() => {
    if (!visible) return;
    // A viewport change invalidates the placement, so re-place on resize.
    const onResize = () => positionMenu(true);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, [visible, positionMenu]);

  // Dismiss on outside click or Escape
  useEffect(() => {
    if (!visible) return;
    const onClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) close();
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') close(); };
    document.addEventListener('mousedown', onClick);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onClick);
      document.removeEventListener('keydown', onKey);
    };
  }, [visible, close]);

  if (!visible || !target || !callbacks) return null;

  const kind = target.kind;
  const hasPlayQueue = kind === 'track' || kind === 'album';
  const hasPlaylist = kind === 'track' || kind === 'album';
  const hasCrossfade = kind === 'album';
  const openPlaylistSubmenu = () => {
    setShowPlaylists(true);
    setShowCrossfade(false);
  };
  const openCrossfadeSubmenu = () => {
    setShowCrossfade(true);
    setShowPlaylists(false);
  };
  const runCustomAction = (action: ContextMenuAction) => {
    if (action.disabled) return;
    close();
    Promise.resolve(action.onSelect()).catch(() => {});
  };

  return createPortal(
    <div
      ref={menuRef}
      style={{ ...CM.menu, left: pos.x, top: pos.y }}
      onContextMenu={e => e.preventDefault()}
    >
      {/* Target label */}
      <div style={CM.targetLabel}>
        <span style={CM.targetKind}>{targetKindLabel(target)}</span>
        <span style={CM.targetTitle}>{targetDisplayName(target)}</span>
      </div>

      <div style={CM.divider} />

      {/* ── Extensible target actions ── */}
      {callbacks.actions?.map((action) => (
        <React.Fragment key={action.id}>
          {action.dividerBefore && <div style={CM.divider} />}
          <button
            type="button"
            disabled={action.disabled}
            style={{ ...CM.item, ...(action.disabled ? CM.itemDisabled : {}) }}
            onClick={() => runCustomAction(action)}
          >
            <ContextActionSVG icon={action.icon} /> {action.label}
          </button>
        </React.Fragment>
      ))}

      {/* ── Track / Album actions ── */}
      {hasPlayQueue && (
        <>
          <button style={CM.item} onClick={() => { callbacks.onPlay?.(); close(); }}>
            <PlaySVG /> {kind === 'album' ? 'Play album' : 'Play'}
          </button>
          <button style={CM.item} onClick={() => { callbacks.onQueue?.(); close(); }}>
            <QueueSVG /> Add to queue
          </button>
        </>
      )}

      {/* ── Track info ── */}
      {kind === 'track' && (
        <button style={CM.item} onClick={() => { openTrackInfo(target.trackId); close(); }}>
          <InfoSVG /> Info
        </button>
      )}

      {/* ── Artist actions ── */}
      {kind === 'artist' && (
        <>
          {callbacks.onPlay && (
            <button style={CM.item} onClick={() => { callbacks.onPlay!(); close(); }}>
              <PlaySVG /> Play artist radio
            </button>
          )}
          {callbacks.onOpen && (
            <button style={CM.item} onClick={() => { callbacks.onOpen!(); close(); }}>
              <ArtistSVG /> Open artist
            </button>
          )}
        </>
      )}

      {/* ── Playlist actions ── */}
      {kind === 'playlist' && (
        <>
          {callbacks.onRename && (
            <button style={CM.item} onClick={() => { callbacks.onRename!(); close(); }}>
              <EditSVG /> Rename
            </button>
          )}
          {callbacks.onDelete && (
            <button style={CM.item} onClick={() => { callbacks.onDelete!(); close(); }}>
              <TrashSVG /> Delete
            </button>
          )}
        </>
      )}

      {/* ── Add to playlist (track & album) ── */}
      {hasPlaylist && (
        <>
          <div style={CM.divider} />
          <button
            style={{ ...CM.item, ...(showPlaylists ? CM.itemActive : {}) }}
            onClick={e => {
              e.stopPropagation();
              if (showPlaylists) {
                setShowPlaylists(false);
                return;
              }
              openPlaylistSubmenu();
            }}
            onMouseEnter={openPlaylistSubmenu}
          >
            <ListSVG /> Add to playlist
            <span style={{ marginLeft: 'auto', opacity: 0.5, fontSize: 12 }}>
              {showPlaylists ? '▲' : '▼'}
            </span>
          </button>
          {showPlaylists && (
            <PlaylistSubmenu target={target} onDone={close} />
          )}
        </>
      )}

      {/* ── Remove from playlist (track in playlist context) ── */}
      {kind === 'track' && callbacks.onRemove && (
        <>
          <div style={CM.divider} />
          <button style={{ ...CM.item, color: '#ef4444' }} onClick={() => { callbacks.onRemove!(); close(); }}>
            <TrashSVG /> Remove from playlist
          </button>
        </>
      )}

      {/* ── Crossfade override (album only) ── */}
      {hasCrossfade && (
        <>
          <div style={CM.divider} />
          <button
            style={{ ...CM.item, ...(showCrossfade ? CM.itemActive : {}) }}
            onClick={e => {
              e.stopPropagation();
              if (showCrossfade) {
                setShowCrossfade(false);
                return;
              }
              openCrossfadeSubmenu();
            }}
            onMouseEnter={openCrossfadeSubmenu}
          >
            <CrossfadeSVG /> Set crossfade…
            <span style={{ marginLeft: 'auto', opacity: 0.5, fontSize: 12 }}>
              {showCrossfade ? '▲' : '▼'}
            </span>
          </button>
          {showCrossfade && (
            <CrossfadeOverridePanel entityType="album" entityId={target.albumId} onDone={close} />
          )}
        </>
      )}
    </div>,
    document.body
  );
}

// ─── Inline SVG icons (tiny, avoids import bloat) ────────────────────────────
const PlaySVG  = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor" style={{ flexShrink: 0 }}><polygon points="5,3 19,12 5,21"/></svg>;
const QueueSVG = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0 }}><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>;
const ListSVG  = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0 }}><line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/></svg>;
const CrossfadeSVG = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0 }}><path d="M2 20V4l10 8L2 20z" opacity="0.6"/><path d="M12 20V4l10 8-10 8z"/></svg>;
const ArtistSVG = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0 }}><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>;
const InfoSVG = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0 }}><circle cx="12" cy="12" r="10"/><line x1="12" y1="11" x2="12" y2="16"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>;
const EditSVG = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0 }}><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>;
const TrashSVG = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0 }}><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>;
const ScanSVG = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0 }}><path d="M20 7v5h-5"/><path d="M18.2 17.2A8 8 0 1 1 20 12"/></svg>;
const CancelSVG = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0 }}><circle cx="12" cy="12" r="9"/><rect x="9" y="9" width="6" height="6" fill="currentColor" stroke="none"/></svg>;
const DeepAnalysisSVG = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0 }}><path d="M2 12h3l2-6 3 12 3-14 3 14 2-6h4"/></svg>;

function ContextActionSVG({ icon }: { icon: ContextActionIcon }) {
  if (icon === 'play') return <PlaySVG />;
  if (icon === 'scan') return <ScanSVG />;
  if (icon === 'cancel') return <CancelSVG />;
  return <DeepAnalysisSVG />;
}

// ─── Styles ───────────────────────────────────────────────────────────────────

const CM: Record<string, React.CSSProperties> = {
  menu: {
    position: 'fixed',
    zIndex: 9999,
    backgroundColor: 'var(--surface)',
    border: '1px solid var(--border)',
    borderRadius: 9,
    minWidth: 220,
    boxShadow: '0 12px 40px rgba(0,0,0,0.55), 0 2px 8px rgba(0,0,0,0.3)',
    // Scroll rather than clip when the menu is taller than the viewport;
    // maxHeight is applied at position time (see positionMenu).
    overflowX: 'hidden',
    overflowY: 'auto',
    fontSize: 15,
    fontFamily: 'var(--font), monospace',
    userSelect: 'none',
  },
  targetLabel: {
    padding: '9px 14px 8px',
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
  },
  targetKind: {
    fontSize: 11,
    fontWeight: 700,
    textTransform: 'uppercase' as const,
    letterSpacing: 1,
    color: 'var(--accent)',
  },
  targetTitle: {
    fontSize: 14,
    fontWeight: 600,
    color: 'var(--text)',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap' as const,
    maxWidth: 200,
    display: 'block',
  },
  divider: {
    height: 1,
    backgroundColor: 'var(--border)',
    margin: '2px 0',
  },
  item: {
    display: 'flex',
    alignItems: 'center',
    gap: 9,
    width: '100%',
    padding: '9px 14px',
    background: 'transparent',
    border: 'none',
    color: 'var(--text)',
    cursor: 'pointer',
    fontSize: 15,
    fontFamily: 'inherit',
    textAlign: 'left' as const,
    transition: 'background 0.1s',
  },
  itemActive: {
    backgroundColor: 'color-mix(in srgb, var(--accent) 10%, transparent)',
    color: 'var(--accent)',
  },
  itemDisabled: {
    color: 'var(--text-muted)',
    cursor: 'not-allowed',
  },
  submenu: {
    borderTop: '1px solid var(--border)',
    maxHeight: 260,
    overflowY: 'auto' as const,
    backgroundColor: 'color-mix(in srgb, var(--bg) 60%, transparent)',
  },
  submenuHeader: {
    padding: '6px 14px',
    fontSize: 12,
    fontWeight: 700,
    textTransform: 'uppercase' as const,
    letterSpacing: 0.8,
    color: 'var(--text-muted)',
  },
  loadingRow: {
    padding: '8px 14px',
    fontSize: 14,
    color: 'var(--text-muted)',
  },
  emptyRow: {
    padding: '6px 14px 8px',
    fontSize: 14,
    color: 'var(--text-muted)',
  },
  plItem: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    width: '100%',
    padding: '8px 14px',
    background: 'transparent',
    border: 'none',
    cursor: 'pointer',
    fontSize: 14,
    fontFamily: 'inherit',
    textAlign: 'left' as const,
  },
  newPlRow: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 6,
    padding: '6px 10px',
    borderTop: '1px solid var(--border)',
  },
  newPlInput: {
    flex: 1,
    backgroundColor: 'var(--bg)',
    border: '1px solid var(--border)',
    color: 'var(--text)',
    borderRadius: 5,
    padding: '5px 8px',
    fontSize: 14,
    fontFamily: 'inherit',
    outline: 'none',
  },
  newPlCreate: {
    backgroundColor: 'var(--accent)',
    color: '#fff',
    border: 'none',
    borderRadius: 5,
    padding: '5px 10px',
    cursor: 'pointer',
    fontSize: 13,
    fontFamily: 'inherit',
    fontWeight: 600,
    flexShrink: 0,
  },
  newPlError: {
    width: '100%',
    color: '#ef4444',
    fontSize: 13,
    lineHeight: 1.3,
  },
};

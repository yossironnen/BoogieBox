/**
 * Defines the Unmerge Modal React component — lets a user with the "Allow
 * metadata editing" permission split some or all of a merged artist's
 * absorbed names back out into their own artist rows. See
 * wip/artist-consolidation-implementation-plan.md.
 */

import React, { useState } from 'react';
import { api } from '../api';
import type { ArtistMergeMember, ClientEntityId, UnmergeResult } from '../types';

interface Props {
  artistId: ClientEntityId;
  artistName: string;
  members: ArtistMergeMember[];
  onClose: () => void;
  onUnmerged: (result: UnmergeResult) => void;
}

/** Unmerge Modal is part of this module's public API. */
export default function UnmergeModal({ artistId, artistName, members, onClose, onUnmerged }: Props) {
  const [checked, setChecked] = useState<Set<ClientEntityId>>(() => new Set(members.map(m => m.id)));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const toggle = (id: ClientEntityId) => {
    setChecked(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  const handleUnmerge = async () => {
    if (checked.size === 0 || saving) return;
    setSaving(true);
    setError(null);
    try {
      const result = await api.unmergeArtist(artistId, Array.from(checked));
      onUnmerged(result);
    } catch (e: any) {
      setError(e?.message || 'Unmerge failed');
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 2000,
        background: 'rgba(0,0,0,0.65)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        padding: 24,
      }}
      onClick={e => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div style={{
        width: '100%', maxWidth: 600,
        maxHeight: '90vh',
        background: 'var(--surface)',
        border: '1px solid var(--border)',
        borderRadius: 12,
        display: 'flex', flexDirection: 'column',
        overflow: 'hidden',
        fontFamily: 'var(--font), monospace',
        color: 'var(--text)',
      }}>
        {/* Header */}
        <div style={{ padding: '18px 24px', borderBottom: '1px solid var(--border)' }}>
          <div style={{ fontSize: 9, fontWeight: 700, letterSpacing: 1.4, textTransform: 'uppercase', color: 'var(--accent)', marginBottom: 3 }}>
            Unmerge Artist
          </div>
          <div style={{ fontSize: 16, fontWeight: 700 }}>Unmerge {artistName}</div>
        </div>

        {/* Body */}
        <div style={{ flex: 1, overflowY: 'auto', padding: '20px 24px', display: 'flex', flexDirection: 'column', gap: 16 }}>
          <div style={{ fontSize: 12.5, color: 'var(--text-muted)', lineHeight: 1.6 }}>
            Splitting a name back out restores it as its own artist and moves its albums/tracks
            back. Uncheck a name to keep it merged.
          </div>

          <div>
            <div style={{ fontSize: 10, fontWeight: 700, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 1.2, marginBottom: 10 }}>
              Merged names
            </div>
            {members.map(m => (
              <label
                key={String(m.id)}
                style={{
                  display: 'flex', alignItems: 'center', gap: 12, padding: '12px 14px',
                  border: '1px solid var(--border)', borderRadius: 8, marginBottom: 8, cursor: 'pointer',
                }}
              >
                <input
                  type="checkbox"
                  checked={checked.has(m.id)}
                  onChange={() => toggle(m.id)}
                  style={{ width: 16, height: 16, accentColor: 'var(--accent)' }}
                />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)' }}>{m.original_name}</div>
                  <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>
                    {m.album_count} {m.album_count === 1 ? 'album' : 'albums'} · {m.track_count} tracks
                  </div>
                </div>
              </label>
            ))}
          </div>

          <div style={{ fontSize: 11.5, color: 'var(--text-muted)', lineHeight: 1.55 }}>
            This does not undo any other edits you made after merging.
          </div>

          {error && <div style={{ fontSize: 12, color: '#f87171' }}>{error}</div>}
        </div>

        {/* Footer */}
        <div style={{ padding: '14px 24px', borderTop: '1px solid var(--border)', display: 'flex', justifyContent: 'flex-end', gap: 10 }}>
          <button
            onClick={onClose}
            style={{ padding: '8px 18px', background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 6, color: 'var(--text)', fontSize: 13, cursor: 'pointer', fontFamily: 'var(--font), monospace' }}
          >
            Cancel
          </button>
          <button
            onClick={handleUnmerge}
            disabled={checked.size === 0 || saving}
            style={{
              padding: '8px 18px',
              background: 'transparent',
              border: '1px solid color-mix(in srgb, #f87171 55%, var(--border))',
              borderRadius: 6,
              color: '#f87171', fontSize: 13, fontWeight: 700,
              cursor: checked.size === 0 || saving ? 'not-allowed' : 'pointer',
              opacity: checked.size === 0 || saving ? 0.6 : 1,
              fontFamily: 'var(--font), monospace',
            }}
          >
            {saving ? 'Unmerging…' : `Unmerge Selected (${checked.size})`}
          </button>
        </div>
      </div>
    </div>
  );
}

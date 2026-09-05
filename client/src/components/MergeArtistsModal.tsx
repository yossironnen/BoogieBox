/**
 * Defines the Merge Artists Modal React component — lets a user with the
 * "Allow metadata editing" permission consolidate 2+ selected duplicate
 * artist rows into one, choosing either an existing artist's name or a
 * custom one. See wip/artist-consolidation-implementation-plan.md.
 */

import React, { useState } from 'react';
import { api } from '../api';
import type { Artist, ClientEntityId } from '../types';

interface Props {
  artists: Artist[];
  onClose: () => void;
  onMerged: (result: Artist) => void;
}

function LockIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0, marginTop: 2, color: 'var(--accent)' }}>
      <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
      <path d="M7 11V7a5 5 0 0 1 10 0v4" />
    </svg>
  );
}

/** Merge Artists Modal is part of this module's public API. */
export default function MergeArtistsModal({ artists, onClose, onMerged }: Props) {
  const [choice, setChoice] = useState<ClientEntityId | 'custom'>(artists[0]?.id ?? 'custom');
  const [customName, setCustomName] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const totalAlbums = artists.reduce((acc, a) => acc + (a.album_count || 0), 0);
  const totalTracks = artists.reduce((acc, a) => acc + (a.track_count || 0), 0);
  const resultName = choice === 'custom'
    ? customName.trim()
    : (artists.find(a => String(a.id) === String(choice))?.name ?? '');
  const canSubmit = artists.length >= 2 && resultName.length > 0 && !saving;

  const handleSave = async () => {
    if (!canSubmit) return;
    setSaving(true);
    setError(null);
    try {
      const masterArtistId = choice === 'custom' ? undefined : choice;
      const result = await api.mergeArtists(artists.map(a => a.id), resultName, masterArtistId);
      onMerged(result);
    } catch (e: any) {
      setError(e?.message || 'Merge failed');
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
          <div style={{ fontSize: 11, fontWeight: 700, letterSpacing: 1.4, textTransform: 'uppercase', color: 'var(--accent)', marginBottom: 3 }}>
            Merge Artists
          </div>
          <div style={{ fontSize: 18, fontWeight: 700 }}>Merge {artists.length} artists into one</div>
          <div style={{ fontSize: 13.5, color: 'var(--text-muted)', marginTop: 3, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {artists.map(a => a.name).join(' · ')}
          </div>
        </div>

        {/* Body */}
        <div style={{ flex: 1, overflowY: 'auto', padding: '20px 24px', display: 'flex', flexDirection: 'column', gap: 16 }}>
          <div>
            <div style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 1.2, marginBottom: 10 }}>
              Choose the artist name
            </div>
            {artists.map(a => (
              <label
                key={String(a.id)}
                style={{
                  display: 'flex', alignItems: 'center', gap: 12, padding: '12px 14px',
                  border: '1px solid', borderColor: choice === a.id ? 'color-mix(in srgb, var(--accent) 45%, var(--border))' : 'var(--border)',
                  background: choice === a.id ? 'color-mix(in srgb, var(--accent) 8%, transparent)' : 'transparent',
                  borderRadius: 8, marginBottom: 8, cursor: 'pointer',
                }}
              >
                <input type="radio" name="merge-master" checked={choice === a.id} onChange={() => setChoice(a.id)} style={{ accentColor: 'var(--accent)' }} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{a.name}</div>
                  <div style={{ fontSize: 13, color: 'var(--text-muted)' }}>
                    {a.album_count} {a.album_count === 1 ? 'album' : 'albums'} · {a.track_count} tracks
                  </div>
                </div>
              </label>
            ))}
            <label
              style={{
                display: 'flex', alignItems: 'center', gap: 10, padding: '12px 14px',
                border: '1px solid', borderColor: choice === 'custom' ? 'color-mix(in srgb, var(--accent) 45%, var(--border))' : 'var(--border)',
                background: choice === 'custom' ? 'color-mix(in srgb, var(--accent) 8%, transparent)' : 'transparent',
                borderRadius: 8, cursor: 'pointer',
              }}
            >
              <input type="radio" name="merge-master" checked={choice === 'custom'} onChange={() => setChoice('custom')} style={{ accentColor: 'var(--accent)' }} />
              <span style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', whiteSpace: 'nowrap' }}>Use a custom name</span>
              <input
                value={customName}
                onChange={e => { setCustomName(e.target.value); setChoice('custom'); }}
                onFocus={() => setChoice('custom')}
                placeholder="Type a new artist name…"
                style={{
                  flex: 1, background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 6,
                  padding: '7px 10px', color: 'var(--text)', fontSize: 14.5, fontFamily: 'inherit',
                }}
              />
            </label>
          </div>

          {resultName && (
            <div style={{
              background: 'color-mix(in srgb, var(--accent) 10%, transparent)',
              border: '1px solid color-mix(in srgb, var(--accent) 28%, var(--border))',
              borderRadius: 8, padding: '12px 14px',
              display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12,
            }}>
              <span style={{ fontSize: 13, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 0.6 }}>Result</span>
              <span style={{ fontSize: 15, color: 'var(--text)', textAlign: 'right' }}>
                <b>{resultName}</b> — {totalAlbums} albums, {totalTracks} tracks
              </span>
            </div>
          )}

          <div style={{ display: 'flex', gap: 8, fontSize: 13.5, color: 'var(--text-muted)', lineHeight: 1.55 }}>
            <LockIcon />
            <span>
              The other names merge into this one — their albums and tracks move over, no
              separation. The name locks so future scans won't split it apart again. You can
              undo this later from the artist page.
              {choice === 'custom' && ' We’ll also search online sources for a match before locking it in.'}
            </span>
          </div>

          {error && <div style={{ fontSize: 14, color: '#f87171' }}>{error}</div>}
        </div>

        {/* Footer */}
        <div style={{ padding: '14px 24px', borderTop: '1px solid var(--border)', display: 'flex', justifyContent: 'flex-end', gap: 10 }}>
          <button
            onClick={onClose}
            style={{ padding: '8px 18px', background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 6, color: 'var(--text)', fontSize: 15, cursor: 'pointer', fontFamily: 'var(--font), monospace' }}
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            disabled={!canSubmit}
            style={{
              padding: '8px 18px',
              background: saving ? 'color-mix(in srgb, var(--accent) 50%, var(--surface))' : 'var(--accent)',
              border: 'none', borderRadius: 6,
              color: '#fff', fontSize: 15, fontWeight: 700,
              cursor: canSubmit ? 'pointer' : 'not-allowed',
              opacity: canSubmit ? 1 : 0.6,
              fontFamily: 'var(--font), monospace',
            }}
          >
            {saving ? 'Merging…' : 'Merge Artists'}
          </button>
        </div>
      </div>
    </div>
  );
}

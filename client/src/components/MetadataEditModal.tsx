/**
 * Defines the Metadata Edit Modal React component and related UI helpers.
 */

import React, { useEffect, useRef, useState } from 'react';
import { api } from '../api';
import type { Album, Artist, ClientEntityId } from '../types';

interface AlbumProps {
  mode: 'album';
  entityId: ClientEntityId;
  initialData: Album;
  onClose: () => void;
  onSaved: () => void;
}

interface ArtistProps {
  mode: 'artist';
  entityId: ClientEntityId;
  initialData: Artist;
  onClose: () => void;
  onSaved: () => void;
}

type Props = AlbumProps | ArtistProps;

const inputStyle: React.CSSProperties = {
  background: 'var(--bg)',
  border: '1px solid var(--border)',
  borderRadius: 6,
  padding: '8px 10px',
  color: 'var(--text)',
  fontSize: 13,
  fontFamily: 'var(--font), monospace',
  width: '100%',
  boxSizing: 'border-box',
};

const labelStyle: React.CSSProperties = {
  fontSize: 10, fontWeight: 700,
  color: 'var(--text-muted)',
  textTransform: 'uppercase',
  letterSpacing: 1,
  display: 'block',
  marginBottom: 6,
};

function LockIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
      <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/>
      <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
    </svg>
  );
}

/** Metadata Edit Modal is part of this module's public API. */
export default function MetadataEditModal({ mode, entityId, initialData, onClose, onSaved }: Props) {
  const isAlbum = mode === 'album';
  const album = isAlbum ? (initialData as Album) : null;
  const artist = !isAlbum ? (initialData as Artist) : null;

  const [title, setTitle]             = useState(album?.title ?? '');
  const [albumArtist, setAlbumArtist] = useState(album?.album_artist ?? '');
  const [year, setYear]               = useState(String(album?.year ?? ''));
  const [genre, setGenre]             = useState(album?.genre ?? '');
  const [releaseType, setReleaseType] = useState<'album' | 'single' | 'compilation'>(album?.releaseType ?? 'album');
  const [name, setName]               = useState(artist?.name ?? '');
  const [description, setDescription] = useState(initialData.description ?? '');

  const [allGenres, setAllGenres]           = useState<string[]>([]);
  const [artworkPreview, setArtworkPreview] = useState<string | null>(null);
  const [artworkBase64, setArtworkBase64]   = useState<string | null>(null);
  const [artworkMime, setArtworkMime]       = useState<string>('image/jpeg');
  const fileInputRef = useRef<HTMLInputElement>(null);

  const [saving, setSaving] = useState(false);
  const [error, setError]   = useState<string | null>(null);
  const isLocked = !!initialData.metadata_locked;

  // Load genre suggestions once on mount
  useEffect(() => {
    api.genres().then(rows => setAllGenres((rows as any[]).map(r => r.genre).filter(Boolean))).catch(() => {});
  }, []);

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setArtworkMime(file.type || 'image/jpeg');
    const reader = new FileReader();
    reader.onload = ev => {
      const dataUrl = ev.target?.result as string;
      setArtworkPreview(dataUrl);
      setArtworkBase64(dataUrl.split(',')[1] ?? null);
    };
    reader.readAsDataURL(file);
  };

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    try {
      if (artworkBase64) {
        if (isAlbum) {
          await api.uploadAlbumArtwork(entityId, artworkBase64, artworkMime);
        } else {
          await api.uploadArtistArtwork(entityId, artworkBase64, artworkMime);
        }
      }

      if (isAlbum) {
        const yearNum = year.trim() ? Number(year.trim()) : undefined;
        await api.updateAlbumMetadata(entityId, {
          title:        title.trim()       || undefined,
          album_artist: albumArtist.trim() !== undefined ? albumArtist.trim() : undefined,
          year:         yearNum,
          genre:        genre.trim()       || undefined,
          description:  description.trim() || undefined,
          releaseType,
        });
      } else {
        if (!name.trim()) { setError('Name is required'); setSaving(false); return; }
        await api.updateArtistMetadata(entityId, {
          name:        name.trim(),
          description: description.trim() || undefined,
        });
      }

      onSaved();
    } catch (e: any) {
      setError(e.message || 'Save failed');
    } finally {
      setSaving(false);
    }
  };

  const currentArtworkUrl = isAlbum ? `/api/albums/${entityId}/cover` : `/api/artists/${entityId}/photo`;
  const genreListId = `genre-suggestions-${entityId}`;

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
        width: '100%', maxWidth: 560,
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
        <div style={{ padding: '18px 24px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
          <div style={{ fontSize: 15, fontWeight: 700 }}>Edit {isAlbum ? 'Album' : 'Artist'}</div>
          <button onClick={onClose} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', fontSize: 18, lineHeight: 1, padding: 4 }}>✕</button>
        </div>

        {/* Body */}
        <div style={{ flex: 1, overflowY: 'auto', padding: '20px 24px', display: 'flex', flexDirection: 'column', gap: 18 }}>

          {/* Lock notice */}
          {isLocked && (
            <div style={{
              display: 'flex', alignItems: 'flex-start', gap: 8,
              background: 'color-mix(in srgb, var(--accent) 12%, var(--surface))',
              border: '1px solid color-mix(in srgb, var(--accent) 35%, transparent)',
              borderRadius: 8, padding: '10px 14px',
              fontSize: 12, color: 'var(--accent)',
            }}>
              <LockIcon />
              <span>Custom metadata — protected from auto-scan. Use <em>Refresh Metadata</em> to re-fetch from providers.</span>
            </div>
          )}

          {/* Artwork */}
          <div>
            <label style={labelStyle}>Artwork</label>
            <div style={{ display: 'flex', alignItems: 'flex-start', gap: 16 }}>
              <div style={{
                width: 100, height: 100, borderRadius: 8, overflow: 'hidden', flexShrink: 0,
                background: 'var(--bg)', border: '1px solid var(--border)',
              }}>
                {artworkPreview
                  ? <img src={artworkPreview} alt="preview" style={{ width: '100%', height: '100%', objectFit: 'cover' }} />
                  : <img src={currentArtworkUrl} alt="current" style={{ width: '100%', height: '100%', objectFit: 'cover' }}
                      onError={e => { (e.currentTarget as HTMLImageElement).style.display = 'none'; }} />
                }
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8, paddingTop: 4 }}>
                <button
                  onClick={() => fileInputRef.current?.click()}
                  style={{ padding: '7px 14px', background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 6, color: 'var(--text)', fontSize: 12, cursor: 'pointer', fontFamily: 'var(--font), monospace' }}
                >
                  Choose image…
                </button>
                {artworkPreview && (
                  <button
                    onClick={() => { setArtworkPreview(null); setArtworkBase64(null); if (fileInputRef.current) fileInputRef.current.value = ''; }}
                    style={{ background: 'none', border: 'none', color: 'var(--text-muted)', fontSize: 11, cursor: 'pointer', textAlign: 'left', padding: 0 }}
                  >
                    Remove selection
                  </button>
                )}
                <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>JPG, PNG or WebP</span>
                <input ref={fileInputRef} type="file" accept="image/*" style={{ display: 'none' }} onChange={handleFileChange} />
              </div>
            </div>
          </div>

          {/* Album fields */}
          {isAlbum && (<>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14 }}>
              <div>
                <label style={labelStyle}>Title</label>
                <input style={inputStyle} value={title} onChange={e => setTitle(e.target.value)} />
              </div>
              <div>
                <label style={labelStyle}>Album Artist</label>
                <input style={inputStyle} value={albumArtist} onChange={e => setAlbumArtist(e.target.value)} />
              </div>
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: '120px 1fr', gap: 14 }}>
              <div>
                <label style={labelStyle}>Year</label>
                <input style={inputStyle} value={year} onChange={e => setYear(e.target.value)} type="number" min="1900" max="2099" />
              </div>
              <div>
                <label style={labelStyle}>Genre</label>
                <input
                  style={inputStyle}
                  value={genre}
                  onChange={e => setGenre(e.target.value)}
                  list={genreListId}
                  autoComplete="off"
                />
                <datalist id={genreListId}>
                  {allGenres.map(g => <option key={g} value={g} />)}
                </datalist>
              </div>
            </div>
            <div>
              <label style={labelStyle}>Release Type</label>
              <select
                style={inputStyle}
                value={releaseType}
                onChange={e => setReleaseType((e.target.value as 'album' | 'single' | 'compilation') ?? 'album')}
              >
                <option value="album">Album</option>
                <option value="single">Single / EP</option>
                <option value="compilation">Compilation</option>
              </select>
            </div>
          </>)}

          {/* Artist name */}
          {!isAlbum && (
            <div>
              <label style={labelStyle}>Name</label>
              <input style={inputStyle} value={name} onChange={e => setName(e.target.value)} />
            </div>
          )}

          {/* Description — both modes */}
          <div>
            <label style={labelStyle}>Description</label>
            <textarea
              style={{ ...inputStyle, resize: 'vertical', minHeight: 80 }}
              value={description}
              onChange={e => setDescription(e.target.value)}
              placeholder="Optional notes or description…"
            />
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
            onClick={handleSave}
            disabled={saving}
            style={{
              padding: '8px 18px',
              background: saving ? 'color-mix(in srgb, var(--accent) 50%, var(--surface))' : 'var(--accent)',
              border: 'none', borderRadius: 6,
              color: '#fff', fontSize: 13, fontWeight: 700,
              cursor: saving ? 'not-allowed' : 'pointer',
              fontFamily: 'var(--font), monospace',
            }}
          >
            {saving ? 'Saving…' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  );
}

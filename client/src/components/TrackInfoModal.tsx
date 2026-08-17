/**
 * Defines the Track Info Modal React component — the kebab menu's "Info"
 * popup, showing full track detail (format, physical path, tags) with
 * inline metadata editing. Mirrors MetadataEditModal's layout/tokens.
 */

import React, { useEffect, useState } from 'react';
import { api } from '../api';
import type { ClientEntityId, Track } from '../types';
import { fmtTrackDur } from './BrowseView';
import { formatBytes } from './SettingsPage';
import { parseTrackTimestamp } from './HomeView';

interface Props {
  trackId: ClientEntityId;
  onClose: () => void;
  onSaved?: () => void;
}

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

const sectionLabelStyle: React.CSSProperties = {
  fontSize: 10, fontWeight: 700,
  color: 'var(--text-muted)',
  textTransform: 'uppercase',
  letterSpacing: 1.2,
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  marginBottom: 10,
};

const kvGridStyle: React.CSSProperties = { display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px 20px' };
const kvKeyStyle: React.CSSProperties = { fontSize: 9.5, fontWeight: 700, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 0.8 };
const kvValStyle: React.CSSProperties = { fontSize: 13, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' };

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div style={sectionLabelStyle}>
      {children}
      <div style={{ flex: 1, height: 1, background: 'var(--border)' }} />
    </div>
  );
}

function KV({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 3, minWidth: 0 }}>
      <span style={kvKeyStyle}>{label}</span>
      <span style={kvValStyle}>{value ?? '—'}</span>
    </div>
  );
}

function CopyIcon() {
  return (
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0 }}>
      <rect x="9" y="9" width="13" height="13" rx="2"/>
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
    </svg>
  );
}

function fmtDateTime(value?: string | null): string {
  const parsed = parseTrackTimestamp(value ?? null);
  return parsed ? parsed.toLocaleString() : (value?.trim() || '—');
}

/** Track Info Modal is part of this module's public API. */
export default function TrackInfoModal({ trackId, onClose, onSaved }: Props) {
  const [track, setTrack]         = useState<Track | null>(null);
  const [loading, setLoading]     = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [title, setTitle]             = useState('');
  const [artist, setArtist]           = useState('');
  const [album, setAlbum]             = useState('');
  const [genre, setGenre]             = useState('');
  const [composer, setComposer]       = useState('');
  const [comment, setComment]         = useState('');
  const [trackNumber, setTrackNumber] = useState('');
  const [discNumber, setDiscNumber]   = useState('');
  const [year, setYear]               = useState('');

  const [allGenres, setAllGenres] = useState<string[]>([]);
  const [saving, setSaving]       = useState(false);
  const [error, setError]         = useState<string | null>(null);
  const [copied, setCopied]       = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setLoadError(null);
    api.track(trackId).then(t => {
      if (cancelled) return;
      setTrack(t);
      setTitle(t.title ?? '');
      setArtist(t.artist ?? '');
      setAlbum(t.album ?? '');
      setGenre(t.genre ?? '');
      setComposer(t.composer ?? '');
      setComment(t.comment ?? '');
      setTrackNumber(t.track_number != null ? String(t.track_number) : '');
      setDiscNumber(t.disc_number != null ? String(t.disc_number) : '');
      setYear(t.year != null ? String(t.year) : '');
      setLoading(false);
    }).catch((e: any) => {
      if (cancelled) return;
      setLoadError(e?.message || 'Could not load track');
      setLoading(false);
    });
    return () => { cancelled = true; };
  }, [trackId]);

  useEffect(() => {
    api.genres().then(rows => setAllGenres((rows as any[]).map(r => r.genre).filter(Boolean))).catch(() => {});
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  const handleCopyPath = () => {
    if (!track?.file_path) return;
    navigator.clipboard?.writeText(track.file_path).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }).catch(() => {});
  };

  const handleSave = async () => {
    if (!title.trim()) { setError('Title is required'); return; }
    setSaving(true);
    setError(null);
    try {
      await api.updateTrackMetadata(trackId, {
        title:       title.trim(),
        artist:      artist.trim()  || undefined,
        album:       album.trim()   || undefined,
        genre:       genre.trim(),
        composer:    composer.trim(),
        comment:     comment.trim(),
        trackNumber: trackNumber.trim() ? Number(trackNumber.trim()) : undefined,
        discNumber:  discNumber.trim()  ? Number(discNumber.trim())  : undefined,
        year:        year.trim()        ? Number(year.trim())        : undefined,
      });
      onSaved?.();
      onClose();
    } catch (e: any) {
      setError(e?.message || 'Save failed');
    } finally {
      setSaving(false);
    }
  };

  const genreListId = `track-genre-suggestions-${trackId}`;
  const bpmDetected = track?.bpm_detected;
  const bpmDiffers = bpmDetected != null && track?.bpm != null && Math.round(bpmDetected) !== Math.round(track.bpm);

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
        width: '100%', maxWidth: 640,
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
        <div style={{ padding: '18px 24px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12 }}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 3, minWidth: 0 }}>
            <span style={{ fontSize: 9, fontWeight: 700, letterSpacing: 1.4, textTransform: 'uppercase', color: 'var(--accent)' }}>Track Info</span>
            <span style={{ fontSize: 16, fontWeight: 700, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{track?.title || 'Loading…'}</span>
            {track && (track.artist || track.album) && (
              <span style={{ fontSize: 11.5, color: 'var(--text-muted)' }}>
                {[track.artist, track.album].filter(Boolean).join(' · ')}
              </span>
            )}
          </div>
          <button onClick={onClose} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', fontSize: 18, lineHeight: 1, padding: 4, flexShrink: 0 }}>✕</button>
        </div>

        {/* Body */}
        <div style={{ flex: 1, overflowY: 'auto', padding: '20px 24px', display: 'flex', flexDirection: 'column', gap: 20 }}>

          {loading && <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>Loading…</div>}
          {loadError && <div style={{ fontSize: 12, color: '#f87171' }}>{loadError}</div>}

          {track && (<>
            {/* File */}
            <div>
              <SectionLabel>File</SectionLabel>
              {track.file_path && (
                <div style={{
                  display: 'flex', alignItems: 'center', gap: 8,
                  background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 7,
                  padding: '9px 12px', marginBottom: 14,
                }}>
                  <span style={{ flex: 1, fontSize: 12, fontFamily: 'var(--font), monospace', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {track.file_path}
                  </span>
                  <button
                    onClick={handleCopyPath}
                    style={{ display: 'flex', alignItems: 'center', gap: 5, background: 'transparent', border: '1px solid var(--border)', borderRadius: 5, color: 'var(--text-muted)', fontSize: 10.5, padding: '4px 8px', cursor: 'pointer', fontFamily: 'var(--font), monospace', flexShrink: 0 }}
                  >
                    <CopyIcon /> {copied ? 'Copied' : 'Copy'}
                  </button>
                </div>
              )}
              <div style={kvGridStyle}>
                <KV label="File Name" value={track.file_name} />
                <KV label="File Size" value={formatBytes(track.file_size)} />
                <KV label="Format" value={track.format} />
                <KV label="Duration" value={fmtTrackDur(track.duration)} />
              </div>
            </div>

            {/* Audio */}
            <div>
              <SectionLabel>Audio</SectionLabel>
              <div style={kvGridStyle}>
                <KV label="Bitrate" value={track.bitrate ? `${track.bitrate.toLocaleString()} kbps` : null} />
                <KV label="Sample Rate" value={track.sample_rate ? `${(track.sample_rate / 1000).toFixed(1)} kHz` : null} />
                <KV label="Channels" value={track.channels ? `${track.channels} (${track.channels === 2 ? 'Stereo' : track.channels === 1 ? 'Mono' : track.channels + 'ch'})` : null} />
                <div style={{ display: 'flex', flexDirection: 'column', gap: 3, minWidth: 0 }}>
                  <span style={kvKeyStyle}>BPM</span>
                  <span style={kvValStyle}>{track.bpm ?? '—'}</span>
                  {(bpmDiffers || track.bpm_source) && (
                    <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 2 }}>
                      {bpmDiffers && (
                        <span style={{ fontSize: 10, fontWeight: 600, color: 'var(--accent)', background: 'var(--bg)', border: '1px solid color-mix(in srgb, var(--accent) 45%, transparent)', borderRadius: 20, padding: '2px 9px' }}>
                          detected {bpmDetected!.toFixed(1)}
                        </span>
                      )}
                      {track.bpm_source && (
                        <span style={{ fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 20, padding: '2px 9px' }}>
                          {track.bpm_source}{track.bpm_confidence != null ? ` · ${Math.round(track.bpm_confidence * 100)}% conf.` : ''}
                        </span>
                      )}
                    </div>
                  )}
                </div>
              </div>
            </div>

            {/* Library */}
            <div>
              <SectionLabel>Library</SectionLabel>
              <div style={kvGridStyle}>
                <KV label="Library" value={track.library_name} />
                <KV label="Scanned" value={fmtDateTime(track.scanned_at)} />
                <KV label="Last Played" value={track.last_played_at ? fmtDateTime(track.last_played_at) : 'Never'} />
                <KV label="Play Count" value={track.play_count ?? 0} />
              </div>
            </div>

            {/* Metadata (editable) */}
            <div>
              <SectionLabel>Metadata</SectionLabel>

              <div style={{
                display: 'flex', alignItems: 'flex-start', gap: 8,
                background: 'color-mix(in srgb, var(--accent) 10%, var(--surface))',
                border: '1px solid color-mix(in srgb, var(--accent) 30%, transparent)',
                borderRadius: 8, padding: '10px 14px', marginBottom: 14,
                fontSize: 11.5, color: 'var(--accent)',
              }}>
                <span>Changes are saved to the library database only — the file&rsquo;s own tags are not modified.</span>
              </div>

              <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14 }}>
                  <div>
                    <label style={labelStyle}>Title</label>
                    <input style={inputStyle} value={title} onChange={e => setTitle(e.target.value)} />
                  </div>
                  <div>
                    <label style={labelStyle}>Artist</label>
                    <input style={inputStyle} value={artist} onChange={e => setArtist(e.target.value)} />
                  </div>
                </div>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14 }}>
                  <div>
                    <label style={labelStyle}>Album</label>
                    <input style={inputStyle} value={album} onChange={e => setAlbum(e.target.value)} />
                  </div>
                  <div>
                    <label style={labelStyle}>Genre</label>
                    <input style={inputStyle} value={genre} onChange={e => setGenre(e.target.value)} list={genreListId} autoComplete="off" />
                    <datalist id={genreListId}>
                      {allGenres.map(g => <option key={g} value={g} />)}
                    </datalist>
                  </div>
                </div>
                <div style={{ display: 'grid', gridTemplateColumns: '100px 100px 1fr', gap: 14 }}>
                  <div>
                    <label style={labelStyle}>Track #</label>
                    <input style={inputStyle} value={trackNumber} onChange={e => setTrackNumber(e.target.value)} type="number" min="0" />
                  </div>
                  <div>
                    <label style={labelStyle}>Disc #</label>
                    <input style={inputStyle} value={discNumber} onChange={e => setDiscNumber(e.target.value)} type="number" min="0" />
                  </div>
                  <div>
                    <label style={labelStyle}>Year</label>
                    <input style={inputStyle} value={year} onChange={e => setYear(e.target.value)} type="number" min="1900" max="2099" />
                  </div>
                </div>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14 }}>
                  <div>
                    <label style={labelStyle}>Composer</label>
                    <input style={inputStyle} value={composer} onChange={e => setComposer(e.target.value)} />
                  </div>
                  <div>
                    <label style={labelStyle}>Comment</label>
                    <input style={inputStyle} value={comment} onChange={e => setComment(e.target.value)} placeholder="Optional notes…" />
                  </div>
                </div>
              </div>
            </div>
          </>)}

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
            disabled={saving || loading || !track}
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

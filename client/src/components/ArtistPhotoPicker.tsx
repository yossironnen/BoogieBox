/**
 * Defines the Artist Photo Picker modal — lets a user browse artist photos
 * found across every configured metadata provider (Deezer, Discogs, Spotify)
 * side by side, or upload their own, instead of silently getting whichever
 * provider answered first.
 */

import React, { useEffect, useRef, useState } from 'react';
import { api } from '../api';
import type { ClientEntityId } from '../types';

const PROVIDER_LABEL: Record<string, string> = {
  deezer: 'Deezer',
  discogs: 'Discogs',
  spotify: 'Spotify',
};

type Selection =
  | { kind: 'candidate'; provider: string; url: string }
  | { kind: 'upload'; base64: string; mime: string; previewUrl: string };

interface Props {
  artistId: ClientEntityId;
  artistName: string;
  onClose: () => void;
  onSelected: () => void;
}

function UploadIcon() {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="17 8 12 3 7 8" /><line x1="12" y1="3" x2="12" y2="15" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

/** Artist Photo Picker is part of this module's public API. */
export default function ArtistPhotoPicker({ artistId, artistName, onClose, onSelected }: Props) {
  const [loading, setLoading] = useState(true);
  const [candidates, setCandidates] = useState<{ provider: string; url: string }[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selection, setSelection] = useState<Selection | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let cancelled = false;
    api.artistPhotoCandidates(artistId)
      .then((res) => { if (!cancelled) setCandidates(res.candidates ?? []); })
      .catch((e: any) => { if (!cancelled) setLoadError(e?.message || 'Could not search providers for this artist.'); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [artistId]);

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const mime = file.type || 'image/jpeg';
    const reader = new FileReader();
    reader.onload = (ev) => {
      const dataUrl = ev.target?.result as string;
      const base64 = dataUrl.split(',')[1] ?? '';
      setSelection({ kind: 'upload', base64, mime, previewUrl: dataUrl });
    };
    reader.readAsDataURL(file);
  };

  const handleUse = async () => {
    if (!selection) return;
    setSaving(true);
    setSaveError(null);
    try {
      if (selection.kind === 'candidate') {
        await api.selectArtistPhoto(artistId, selection.url, selection.provider);
      } else {
        await api.uploadArtistArtwork(artistId, selection.base64, selection.mime);
      }
      onSelected();
      onClose();
    } catch (e: any) {
      setSaveError(e.message || 'Could not save this photo');
    } finally {
      setSaving(false);
    }
  };

  const currentPhotoUrl = api.artistPhotoUrl(artistId, 300);

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 2000,
        background: 'rgba(0,0,0,0.65)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        padding: 24,
      }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
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
        <div style={{ padding: '18px 24px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
          <div>
            <div style={{ fontSize: 17, fontWeight: 700 }}>Choose Artist Photo</div>
            <div style={{ fontSize: 13, color: 'var(--text-muted)', marginTop: 2 }}>{artistName}</div>
          </div>
          <button onClick={onClose} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', fontSize: 20, lineHeight: 1, padding: 4 }}>✕</button>
        </div>

        <div style={{ flex: 1, overflowY: 'auto', padding: '20px 24px' }}>
          {loading && <div style={{ padding: '30px 0', textAlign: 'center', color: 'var(--text-muted)', fontSize: 14 }}>Searching providers…</div>}
          {!loading && loadError && (
            <div style={{ padding: '20px 0', textAlign: 'center', color: 'var(--text-muted)', fontSize: 14 }}>{loadError}</div>
          )}
          {!loading && !loadError && (
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(110px, 1fr))', gap: 14 }}>
              <div style={{ border: '2px solid var(--border)', borderRadius: 10, overflow: 'hidden', background: 'var(--bg)' }}>
                <div style={{ width: '100%', aspectRatio: '1', overflow: 'hidden' }}>
                  <img src={currentPhotoUrl} alt="Current" style={{ width: '100%', height: '100%', objectFit: 'cover', display: 'block' }}
                    onError={(e) => { (e.currentTarget as HTMLImageElement).style.visibility = 'hidden'; }} />
                </div>
                <div style={{ padding: '6px 8px', fontSize: 11, color: 'var(--text-muted)' }}>Current</div>
              </div>

              {candidates.map((c) => {
                const isSelected = selection?.kind === 'candidate' && selection.provider === c.provider && selection.url === c.url;
                return (
                  <button
                    key={c.provider}
                    type="button"
                    onClick={() => setSelection({ kind: 'candidate', provider: c.provider, url: c.url })}
                    style={{
                      border: isSelected ? '2px solid var(--accent)' : '2px solid var(--border)',
                      boxShadow: isSelected ? '0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent)' : 'none',
                      borderRadius: 10, overflow: 'hidden', background: 'var(--bg)', padding: 0, cursor: 'pointer',
                      position: 'relative', fontFamily: 'inherit', textAlign: 'left',
                    }}
                  >
                    <div style={{ width: '100%', aspectRatio: '1', overflow: 'hidden' }}>
                      <img src={c.url} alt={PROVIDER_LABEL[c.provider] ?? c.provider} style={{ width: '100%', height: '100%', objectFit: 'cover', display: 'block' }} />
                    </div>
                    <div style={{ padding: '6px 8px', fontSize: 11, color: 'var(--text-muted)' }}>{PROVIDER_LABEL[c.provider] ?? c.provider}</div>
                    {isSelected && (
                      <span style={{
                        position: 'absolute', top: 6, right: 6, width: 20, height: 20, borderRadius: 999,
                        background: 'var(--accent)', color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center',
                      }}>
                        <CheckIcon />
                      </span>
                    )}
                  </button>
                );
              })}

              {!candidates.length && (
                <div style={{ gridColumn: '1 / -1', fontSize: 13, color: 'var(--text-muted)', padding: '4px 0 8px' }}>
                  No matches found from your configured providers. Add a Discogs or Spotify key in Settings to search more sources, or upload your own below.
                </div>
              )}

              <button
                type="button"
                onClick={() => fileInputRef.current?.click()}
                style={{
                  border: selection?.kind === 'upload' ? '2px solid var(--accent)' : '2px dashed var(--border)',
                  boxShadow: selection?.kind === 'upload' ? '0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent)' : 'none',
                  borderRadius: 10, background: 'transparent', cursor: 'pointer',
                  display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 6,
                  color: 'var(--text-muted)', fontSize: 11.5, textAlign: 'center', padding: 8,
                  aspectRatio: '1', fontFamily: 'inherit', overflow: 'hidden', position: 'relative',
                }}
              >
                {selection?.kind === 'upload'
                  ? <img src={selection.previewUrl} alt="Upload preview" style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', objectFit: 'cover' }} />
                  : <><UploadIcon />Upload your own</>}
                {selection?.kind === 'upload' && (
                  <span style={{
                    position: 'absolute', top: 6, right: 6, width: 20, height: 20, borderRadius: 999,
                    background: 'var(--accent)', color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center',
                  }}>
                    <CheckIcon />
                  </span>
                )}
              </button>
              <input ref={fileInputRef} type="file" accept="image/*" style={{ display: 'none' }} onChange={handleFileChange} />
            </div>
          )}
          {saveError && <div style={{ marginTop: 14, fontSize: 13, color: '#f37272' }}>{saveError}</div>}
        </div>

        <div style={{ padding: '14px 24px', borderTop: '1px solid var(--border)', display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <button
            onClick={onClose}
            style={{ padding: '8px 16px', background: 'transparent', border: '1px solid var(--border)', borderRadius: 6, color: 'var(--text-muted)', fontSize: 13.5, cursor: 'pointer', fontFamily: 'inherit' }}
          >
            Cancel
          </button>
          <button
            onClick={handleUse}
            disabled={!selection || saving}
            style={{
              padding: '8px 16px', background: 'var(--accent)', border: '1px solid transparent', borderRadius: 6,
              color: '#fff', fontSize: 13.5, fontWeight: 600, fontFamily: 'inherit',
              cursor: !selection || saving ? 'not-allowed' : 'pointer',
              opacity: !selection || saving ? 0.6 : 1,
            }}
          >
            {saving ? 'Saving…' : 'Use this photo'}
          </button>
        </div>
      </div>
    </div>
  );
}

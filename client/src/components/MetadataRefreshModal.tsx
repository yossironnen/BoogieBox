/**
 * Defines the Metadata Refresh Modal React component and related UI helpers.
 */

import React, { useState } from 'react';
import { api } from '../api';
import type { ClientEntityId, MetadataSearchResult, ProviderSearchWarning } from '../types';

interface Props {
  mode: 'artist' | 'album';
  entityId: ClientEntityId;
  initialArtist: string;
  initialAlbum?: string;
  onClose: () => void;
  onApplied: (mergedIntoId?: ClientEntityId) => void;
}

const PROVIDER_LABELS: Record<string, string> = {
  lastfm: 'Last.fm',
  discogs: 'Discogs',
  spotify: 'Spotify',
  deezer: 'Deezer',
};

const PROVIDER_COLORS: Record<string, string> = {
  lastfm: '#d51007',
  discogs: '#333',
  spotify: '#1db954',
  deezer: '#a238ff',
};

export function normalizeMetadataYear(year: MetadataSearchResult['year']): number | undefined {
  if (typeof year === 'number' && Number.isFinite(year)) return Math.trunc(year);
  if (typeof year === 'string') {
    const match = year.trim().match(/^\d{4}/);
    if (match) return Number(match[0]);
  }
  return undefined;
}

/** Metadata Refresh Modal is part of this module's public API. */
export default function MetadataRefreshModal({ mode, entityId, initialArtist, initialAlbum, onClose, onApplied }: Props) {
  const [artist, setArtist] = useState(initialArtist);
  const [album, setAlbum] = useState(initialAlbum ?? '');
  const [results, setResults] = useState<MetadataSearchResult[] | null>(null);
  const [providerWarnings, setProviderWarnings] = useState<ProviderSearchWarning[]>([]);
  const [loading, setLoading] = useState(false);
  const [applying, setApplying] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const runWithTimeout = async (task: Promise<unknown>, timeoutMs: number): Promise<void> => {
    await Promise.race([
      task.then(() => undefined).catch(() => undefined),
      new Promise<void>((resolve) => setTimeout(resolve, timeoutMs)),
    ]);
  };

  const handleSearch = async () => {
    if (!artist.trim()) return;
    setLoading(true);
    setError(null);
    setResults(null);
    setProviderWarnings([]);
    try {
      const resp = await api.integrations.metadataSearch({
        artist: artist.trim(),
        album: mode === 'album' && album.trim() ? album.trim() : undefined,
      });
      setResults(resp.results);
      setProviderWarnings(resp.provider_warnings ?? []);
    } catch (err: any) {
      setError(err.message || 'Search failed');
    } finally {
      setLoading(false);
    }
  };

  const handleApply = async (result: MetadataSearchResult) => {
    const key = `${result.provider}-${result.title}`;
    setApplying(key);
    setError(null);
    try {
      if (mode === 'album') {
        const resp = await api.updateAlbumMetadata(entityId, {
          title: result.title,
          album_artist: result.artist,
          year: normalizeMetadataYear(result.year),
          genre: result.genre ?? result.tags?.[0],
          releaseType: result.releaseType,
          discogsReleaseType: result.provider === 'discogs' ? result.releaseType : undefined,
          spotifyReleaseType: result.provider === 'spotify' ? result.releaseType : undefined,
        }, true);
        const effectiveId = resp.merged_into ?? entityId;
        void runWithTimeout(api.refreshAlbumCover(effectiveId), 3000);
        onApplied(resp.merged_into);
      } else {
        await api.updateArtistMetadata(entityId, { name: result.title }, true);
        void runWithTimeout(api.refreshArtistPhoto(entityId), 3000);
        onApplied();
      }
    } catch (err: any) {
      setError(err.message || 'Apply failed');
    } finally {
      setApplying(null);
    }
  };

  const providers = results ? [...new Set(results.map(r => r.provider))] : [];

  const inputStyle: React.CSSProperties = {
    background: 'var(--bg)',
    border: '1px solid var(--border)',
    borderRadius: 6,
    padding: '8px 10px',
    color: 'var(--text)',
    fontSize: 15,
    fontFamily: 'var(--font), monospace',
    width: '100%',
    boxSizing: 'border-box',
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
        width: '100%', maxWidth: 680,
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
          <div style={{ fontSize: 17, fontWeight: 700 }}>Refresh Metadata</div>
          <button
            onClick={onClose}
            style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', fontSize: 20, lineHeight: 1, padding: 4 }}
          >✕</button>
        </div>

        {/* Search fields */}
        <div style={{ padding: '18px 24px', borderBottom: '1px solid var(--border)' }}>
          <div style={{ display: 'grid', gridTemplateColumns: mode === 'album' ? '1fr 1fr' : '1fr', gap: 12, marginBottom: 12 }}>
            <div>
              <label style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 1, display: 'block', marginBottom: 6 }}>
                Artist
              </label>
              <input style={inputStyle} value={artist} onChange={e => setArtist(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && handleSearch()} />
            </div>
            {mode === 'album' && (
              <div>
                <label style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 1, display: 'block', marginBottom: 6 }}>
                  Album
                </label>
                <input style={inputStyle} value={album} onChange={e => setAlbum(e.target.value)}
                  onKeyDown={e => e.key === 'Enter' && handleSearch()} />
              </div>
            )}
          </div>
          <button
            onClick={handleSearch}
            disabled={loading || !artist.trim()}
            style={{
              padding: '8px 20px',
              background: artist.trim() && !loading ? 'var(--accent)' : 'color-mix(in srgb, var(--accent) 35%, var(--surface))',
              border: 'none', borderRadius: 6,
              color: '#fff', fontSize: 14, fontWeight: 700,
              cursor: loading || !artist.trim() ? 'not-allowed' : 'pointer',
              fontFamily: 'var(--font), monospace',
            }}
          >
            {loading ? 'Searching…' : 'Search'}
          </button>
          {error && (
            <div style={{ marginTop: 10, fontSize: 14, color: '#f87171' }}>{error}</div>
          )}
        </div>

        {/* Results */}
        <div style={{ flex: 1, overflowY: 'auto', padding: '18px 24px' }}>
          {results === null && !loading && (
            <div style={{ color: 'var(--text-muted)', fontSize: 15, textAlign: 'center', paddingTop: 24 }}>
              Enter search terms above and click Search.
            </div>
          )}
          {providerWarnings.some(w => w.reason === 'rate_limited') && (
            <div style={{
              display: 'flex', alignItems: 'flex-start', gap: 8,
              background: 'color-mix(in srgb, #f59e0b 12%, transparent)',
              border: '1px solid color-mix(in srgb, #f59e0b 40%, var(--border))',
              borderRadius: 8, padding: '10px 12px', marginBottom: 14,
              fontSize: 14, color: 'var(--text)', lineHeight: 1.5,
            }}>
              <span aria-hidden="true">⚠️</span>
              <span>Metadata provider rate limit reached. This is common on free API tiers — please try again in a few minutes.</span>
            </div>
          )}
          {results !== null && results.length === 0 && (
            <div style={{ color: 'var(--text-muted)', fontSize: 15, textAlign: 'center', paddingTop: 24 }}>
              No results found.
            </div>
          )}
          {providers.map(provider => {
            const providerResults = results!.filter(r => r.provider === provider);
            return (
              <div key={provider} style={{ marginBottom: 24 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
                  <span style={{
                    fontSize: 12, fontWeight: 700, textTransform: 'uppercase', letterSpacing: 1,
                    color: '#fff',
                    background: PROVIDER_COLORS[provider] ?? '#555',
                    borderRadius: 4, padding: '2px 7px',
                  }}>
                    {PROVIDER_LABELS[provider] ?? provider}
                  </span>
                  <span style={{ fontSize: 13, color: 'var(--text-muted)' }}>{providerResults.length} result{providerResults.length !== 1 ? 's' : ''}</span>
                </div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                  {providerResults.map((r, i) => {
                    const key = `${r.provider}-${r.title}-${i}`;
                    const isApplying = applying === `${r.provider}-${r.title}`;
                    return (
                      <div
                        key={key}
                        style={{
                          display: 'flex', alignItems: 'center', gap: 12,
                          background: 'var(--bg)',
                          border: '1px solid var(--border)',
                          borderRadius: 8, padding: '10px 12px',
                        }}
                      >
                        {r.image ? (
                          <img src={r.image} alt="" style={{ width: 48, height: 48, borderRadius: 6, objectFit: 'cover', flexShrink: 0 }} />
                        ) : (
                          <div style={{ width: 48, height: 48, borderRadius: 6, background: 'var(--border)', flexShrink: 0 }} />
                        )}
                        <div style={{ flex: 1, minWidth: 0 }}>
                          <div style={{ fontWeight: 700, fontSize: 15, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{r.title}</div>
                          {r.artist && <div style={{ fontSize: 13, color: 'var(--text-muted)', marginTop: 2 }}>{r.artist}</div>}
                          <div style={{ fontSize: 13, color: 'var(--text-muted)', marginTop: 2, display: 'flex', gap: 8 }}>
                            {r.year && <span>{r.year}</span>}
                            {r.genre && <span>{r.genre}</span>}
                            {r.tags?.slice(0, 3).map(t => <span key={t}>{t}</span>)}
                          </div>
                        </div>
                        <button
                          onClick={() => handleApply(r)}
                          disabled={isApplying}
                          style={{
                            padding: '6px 14px',
                            background: 'var(--accent)',
                            border: 'none', borderRadius: 6,
                            color: '#fff', fontSize: 13, fontWeight: 700,
                            cursor: isApplying ? 'wait' : 'pointer',
                            flexShrink: 0,
                            fontFamily: 'var(--font), monospace',
                          }}
                        >
                          {isApplying ? '…' : 'Apply'}
                        </button>
                      </div>
                    );
                  })}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

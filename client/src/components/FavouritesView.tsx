/**
 * Browse → Favourites: the signed-in user's favourite artists, albums and songs (hearts), newest first.
 * They are the same as the stars set in Subsonic apps.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { api } from '../api';
import ArtImage from './ArtImage';
import { KebabButton } from './ContextMenu';
import { HeartIcon, HeartToggle, SongIcon } from './FavouriteIcons';
import { useFavourites, type Favourites } from '../favourites';
import type { Album, Artist, QueueSource, Track } from '../types';

export type FavouritesSection = 'artists' | 'albums' | 'songs';

interface Props {
  playTrack: (track: Track, allTracks?: Track[], source?: QueueSource) => void;
  addToQueue: (track: Track) => void;
  onOpenAlbum: (album: Album) => void;
  onOpenArtist: (artist: Artist) => void;
  onPlayAlbum: (album: Album) => void;
  onPlayArtist: (artist: Artist) => void;
  /** Section to show first. */
  initialSection?: FavouritesSection;
}

const ArtistGlyph = ({ size = 15 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" /><circle cx="12" cy="7" r="4" />
  </svg>
);
const AlbumGlyph = ({ size = 15 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
    <circle cx="12" cy="12" r="10" /><circle cx="12" cy="12" r="3" />
  </svg>
);
const PlayGlyph = ({ size = 14 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><polygon points="6,4 20,12 6,20" /></svg>
);

const EMPTY: Favourites = { artists: [], albums: [], tracks: [] };

/** `m:ss` for a track length. */
export function formatDuration(seconds: number | null | undefined): string {
  if (!seconds || !Number.isFinite(seconds) || seconds <= 0) return '';
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${String(s).padStart(2, '0')}`;
}

/** Rows still hearted (un-hearting elsewhere hides them at once). */
export function visibleFavourites(data: Favourites, has: (kind: 'track' | 'album' | 'artist', id: string) => boolean): Favourites {
  return {
    artists: data.artists.filter((a) => has('artist', String(a.id))),
    albums: data.albums.filter((a) => has('album', String(a.id))),
    tracks: data.tracks.filter((t) => has('track', String(t.id))),
  };
}

export default function FavouritesView({
  playTrack,
  addToQueue,
  onOpenAlbum,
  onOpenArtist,
  onPlayAlbum,
  onPlayArtist,
  initialSection = 'songs',
}: Props) {
  const favourites = useFavourites();
  const [section, setSection] = useState<FavouritesSection>(initialSection);
  const [data, setData] = useState<Favourites>(EMPTY);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setLoading(true);
    setError(null);
    api.favourites.list()
      .then((result) => setData(result))
      .catch((err: unknown) => setError(err instanceof Error ? err.message : 'Could not load favourites'))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => { load(); }, [load]);

  const shown = visibleFavourites(data, favourites.has);
  const counts = { artists: shown.artists.length, albums: shown.albums.length, songs: shown.tracks.length };
  const source: QueueSource = { type: 'favourites', id: '0' };

  const sections: Array<{ id: FavouritesSection; label: string; icon: React.ReactNode }> = [
    { id: 'songs', label: 'Songs', icon: <SongIcon size={15} /> },
    { id: 'albums', label: 'Albums', icon: <AlbumGlyph /> },
    { id: 'artists', label: 'Artists', icon: <ArtistGlyph /> },
  ];

  return (
    <div style={S.root}>
      <div style={S.toolbar}>
        <div style={S.seg} role="group" aria-label="Favourites">
          {sections.map((s) => (
            <button
              key={s.id}
              type="button"
              style={{ ...S.segBtn, ...(section === s.id ? S.segBtnActive : {}) }}
              aria-pressed={section === s.id}
              onClick={() => setSection(s.id)}
            >
              {s.icon}
              {s.label}
              <span style={S.count}>{counts[s.id].toLocaleString()}</span>
            </button>
          ))}
        </div>
        {section === 'songs' && shown.tracks.length > 0 && (
          <button
            type="button"
            className="icon-action-btn"
            style={S.playAll}
            title="Play all favourite songs"
            aria-label="Play all favourite songs"
            onClick={() => playTrack(shown.tracks[0], shown.tracks, source)}
          >
            <PlayGlyph size={16} />
          </button>
        )}
      </div>

      {error && <div role="alert" style={S.notice}>{error}</div>}
      {loading && !error && <div style={S.notice}>Loading…</div>}

      {!loading && !error && section === 'songs' && (
        shown.tracks.length === 0 ? <Empty text="No favourite songs yet" /> : (
          <div style={S.list} role="list">
            {shown.tracks.map((track) => (
              <div
                key={String(track.id)}
                role="listitem"
                style={S.row}
                onDoubleClick={() => playTrack(track, shown.tracks, source)}
              >
                <HeartToggle
                  on={favourites.has('track', String(track.id))}
                  label={track.title || track.file_name}
                  onToggle={() => { void favourites.toggle('track', track.id).catch(() => {}); }}
                />
                <button type="button" style={S.rowPlay} aria-label={`Play ${track.title || track.file_name}`} onClick={() => playTrack(track, shown.tracks, source)}>
                  <PlayGlyph size={11} />
                </button>
                <div style={S.rowText}>
                  <div style={S.rowTitle}>{track.title || track.file_name}</div>
                  <div style={S.rowMeta}>{[track.artist, track.album].filter(Boolean).join(' — ')}</div>
                </div>
                <span style={S.duration}>{formatDuration(track.duration)}</span>
                <KebabButton
                  target={{ kind: 'track', trackId: track.id, title: track.title || track.file_name }}
                  callbacks={{ onPlay: () => playTrack(track, shown.tracks, source), onQueue: () => addToQueue(track) }}
                />
              </div>
            ))}
          </div>
        )
      )}

      {!loading && !error && section === 'albums' && (
        shown.albums.length === 0 ? <Empty text="No favourite albums yet" /> : (
          <div style={S.grid}>
            {shown.albums.map((album) => (
              <div key={String(album.id)} style={S.card} role="button" tabIndex={0}
                onClick={() => onOpenAlbum(album)}
                onKeyDown={(e) => { if (e.key === 'Enter') onOpenAlbum(album); }}
              >
                <div style={S.artWrap}>
                  <ArtImage src={api.albumArtUrl(album.id, 300)} alt={album.title} imgStyle={S.art} fallback={<div style={S.artFallback}><AlbumGlyph size={36} /></div>} />
                  <button type="button" style={S.cardPlay} aria-label={`Play ${album.title}`} title="Play"
                    onClick={(e) => { e.stopPropagation(); onPlayAlbum(album); }}
                  >
                    <PlayGlyph />
                  </button>
                  <HeartToggle
                    on={favourites.has('album', String(album.id))}
                    label={album.title}
                    onToggle={() => { void favourites.toggle('album', album.id).catch(() => {}); }}
                    style={S.cardHeart}
                  />
                </div>
                <div style={S.cardTitle}>{album.title}</div>
                <div style={S.cardMeta}>{album.album_artist || album.artist || ''}</div>
              </div>
            ))}
          </div>
        )
      )}

      {!loading && !error && section === 'artists' && (
        shown.artists.length === 0 ? <Empty text="No favourite artists yet" /> : (
          <div style={S.grid}>
            {shown.artists.map((artist) => (
              <div key={String(artist.id)} style={S.card} role="button" tabIndex={0}
                onClick={() => onOpenArtist(artist)}
                onKeyDown={(e) => { if (e.key === 'Enter') onOpenArtist(artist); }}
              >
                <div style={{ ...S.artWrap, borderRadius: '50%' }}>
                  <ArtImage src={api.artistPhotoUrl(artist.id, 300)} alt={artist.name} imgStyle={{ ...S.art, borderRadius: '50%' }} fallback={<div style={{ ...S.artFallback, borderRadius: '50%' }}><ArtistGlyph size={36} /></div>} />
                  <button type="button" style={S.cardPlay} aria-label={`Play ${artist.name} radio`} title="Play artist radio"
                    onClick={(e) => { e.stopPropagation(); onPlayArtist(artist); }}
                  >
                    <PlayGlyph />
                  </button>
                </div>
                <div style={{ ...S.cardTitle, textAlign: 'center' }}>
                  {artist.name}
                </div>
                <div style={{ display: 'flex', justifyContent: 'center' }}>
                  <HeartToggle
                    on={favourites.has('artist', String(artist.id))}
                    label={artist.name}
                    onToggle={() => { void favourites.toggle('artist', artist.id).catch(() => {}); }}
                  />
                </div>
              </div>
            ))}
          </div>
        )
      )}
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return (
    <div style={S.empty}>
      <span style={{ color: 'var(--accent)', opacity: 0.85 }}><HeartIcon size={32} /></span>
      <div>{text}</div>
    </div>
  );
}

const S: Record<string, React.CSSProperties> = {
  root: { display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0, overflowY: 'auto', padding: '12px 20px 24px' },
  toolbar: { display: 'flex', alignItems: 'center', gap: 12, marginBottom: 14, flexWrap: 'wrap' },
  seg: { display: 'inline-flex', padding: 3, borderRadius: 11, border: '1px solid var(--border)', background: 'color-mix(in srgb, var(--surface) 84%, var(--bg))' },
  segBtn: { display: 'inline-flex', alignItems: 'center', gap: 7, padding: '6px 12px', borderRadius: 8, border: 'none', background: 'transparent', color: 'var(--text-muted)', cursor: 'pointer', fontFamily: 'inherit', fontSize: 14, fontWeight: 600 },
  segBtnActive: { background: 'color-mix(in srgb, var(--accent) 20%, transparent)', color: 'var(--text)' },
  count: { fontSize: 12, fontWeight: 500, fontVariantNumeric: 'tabular-nums', opacity: 0.8 },
  playAll: { width: 36, height: 36, borderRadius: '50%', border: 'none', background: 'var(--accent)', color: 'var(--bg)', cursor: 'pointer', display: 'inline-flex', alignItems: 'center', justifyContent: 'center' },
  notice: { color: 'var(--text-muted)', padding: '12px 0' },
  list: { display: 'flex', flexDirection: 'column', gap: 2 },
  row: { display: 'flex', alignItems: 'center', gap: 8, padding: '6px 8px', borderRadius: 8, background: 'color-mix(in srgb, var(--surface) 60%, transparent)' },
  rowPlay: { width: 26, height: 26, borderRadius: '50%', border: '1px solid var(--border)', background: 'transparent', color: 'var(--text)', cursor: 'pointer', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
  rowText: { flex: 1, minWidth: 0 },
  rowTitle: { fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  rowMeta: { fontSize: 12, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  duration: { fontSize: 12, color: 'var(--text-muted)', fontVariantNumeric: 'tabular-nums', flexShrink: 0 },
  grid: { display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))', gap: 16 },
  card: { display: 'flex', flexDirection: 'column', gap: 6, cursor: 'pointer', minWidth: 0 },
  artWrap: { position: 'relative', aspectRatio: '1 / 1', borderRadius: 12, overflow: 'hidden', background: 'var(--surface)' },
  art: { width: '100%', height: '100%', objectFit: 'cover', display: 'block' },
  artFallback: { width: '100%', height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)' },
  cardPlay: { position: 'absolute', right: 8, bottom: 8, width: 34, height: 34, borderRadius: '50%', border: 'none', background: 'var(--accent)', color: 'var(--bg)', cursor: 'pointer', display: 'inline-flex', alignItems: 'center', justifyContent: 'center' },
  cardHeart: { position: 'absolute', left: 6, top: 6, background: 'color-mix(in srgb, var(--bg) 70%, transparent)' },
  cardTitle: { fontWeight: 600, fontSize: 14, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  cardMeta: { fontSize: 12, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  empty: { display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 10, padding: '48px 0', color: 'var(--text-muted)' },
};

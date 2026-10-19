/**
 * Home → Favourites: counts, the latest favourite albums and songs, and a shortcut to Browse → Favourites.
 */

import React, { useEffect, useRef, useState } from 'react';
import { api } from '../api';
import ArtImage from './ArtImage';
import { HeartIcon, HeartToggle, SongIcon } from './FavouriteIcons';
import { favouritesStore, useFavourites, type Favourites } from '../favourites';
import { visibleFavourites } from './FavouritesView';
import type { Album, QueueSource, Track } from '../types';

const LIMIT = 12;
const EMPTY: Favourites = { artists: [], albums: [], tracks: [] };

const AlbumGlyph = ({ size = 14 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
    <circle cx="12" cy="12" r="10" /><circle cx="12" cy="12" r="3" />
  </svg>
);
const ArtistGlyph = ({ size = 14 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
    <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" /><circle cx="12" cy="7" r="4" />
  </svg>
);
const PlayGlyph = ({ size = 11 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><polygon points="6,4 20,12 6,20" /></svg>
);

export default function FavouritesWidget({
  onOpenAlbum,
  onPlayTrack,
  onOpenFavourites,
}: {
  onOpenAlbum: (album: Album) => void;
  onPlayTrack: (track: Track, allTracks?: Track[], source?: QueueSource) => void;
  onOpenFavourites: () => void;
}) {
  const favourites = useFavourites();
  const [data, setData] = useState<Favourites>(EMPTY);
  const [loaded, setLoaded] = useState(false);
  const version = favouritesStore.getVersion();
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Refresh (debounced) whenever a heart changes anywhere.
  useEffect(() => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      api.favourites.list(LIMIT)
        .then(setData)
        .catch(() => {})
        .finally(() => setLoaded(true));
    }, loaded ? 400 : 0);
    return () => { if (timer.current) clearTimeout(timer.current); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version]);

  const shown = visibleFavourites(data, favourites.has);
  const source: QueueSource = { type: 'favourites', id: '0' };
  const empty = loaded && !shown.tracks.length && !shown.albums.length && !shown.artists.length;

  return (
    <div style={W.root}>
      <div style={W.stats}>
        <span style={W.stat}><span style={W.statIcon}><SongIcon size={15} /></span>{favourites.count('track').toLocaleString()} songs</span>
        <span style={W.stat}><span style={W.statIcon}><AlbumGlyph size={15} /></span>{favourites.count('album').toLocaleString()} albums</span>
        <span style={W.stat}><span style={W.statIcon}><ArtistGlyph size={15} /></span>{favourites.count('artist').toLocaleString()} artists</span>
        <button type="button" style={W.open} onClick={onOpenFavourites}>
          <HeartIcon size={13} /> Open Favourites
        </button>
      </div>

      {empty && (
        <div style={W.empty}>
          <span style={{ color: 'var(--accent)', opacity: 0.85 }}><HeartIcon size={26} /></span>
          Use “Add to Favourites” on any song, album or artist to see it here.
        </div>
      )}

      {shown.albums.length > 0 && (
        <div style={W.albums}>
          {shown.albums.slice(0, 6).map((album) => (
            <button key={String(album.id)} type="button" style={W.album} onClick={() => onOpenAlbum(album)} title={album.title}>
              <ArtImage src={api.albumArtUrl(album.id, 300)} alt={album.title} imgStyle={W.art} wrapperStyle={W.artWrap} fallback={<div style={W.artFallback}><AlbumGlyph size={28} /></div>} />
              <span style={W.albumTitle}>{album.title}</span>
            </button>
          ))}
        </div>
      )}

      {shown.tracks.length > 0 && (
        <div style={W.tracks}>
          {shown.tracks.slice(0, 5).map((track) => (
            <div key={String(track.id)} style={W.track}>
              <button type="button" style={W.trackPlay} aria-label={`Play ${track.title || track.file_name}`}
                onClick={() => onPlayTrack(track, shown.tracks, source)}
              >
                <PlayGlyph />
              </button>
              <div style={W.trackText}>
                <div style={W.trackTitle}>{track.title || track.file_name}</div>
                <div style={W.trackMeta}>{track.artist || ''}</div>
              </div>
              <HeartToggle
                on={favourites.has('track', String(track.id))}
                label={track.title || track.file_name}
                onToggle={() => { void favourites.toggle('track', track.id).catch(() => {}); }}
              />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

const W: Record<string, React.CSSProperties> = {
  root: { display: 'flex', flexDirection: 'column', gap: 14 },
  stats: { display: 'flex', alignItems: 'center', gap: 16, flexWrap: 'wrap' },
  stat: { display: 'inline-flex', alignItems: 'center', gap: 6, fontWeight: 600, fontVariantNumeric: 'tabular-nums' },
  statIcon: { color: 'var(--accent)', opacity: 0.85, display: 'inline-flex' },
  open: { marginLeft: 'auto', display: 'inline-flex', alignItems: 'center', gap: 6, padding: '6px 12px', borderRadius: 8, border: '1px solid var(--border)', background: 'transparent', color: 'var(--text)', cursor: 'pointer', fontFamily: 'inherit', fontWeight: 600 },
  empty: { display: 'flex', alignItems: 'center', gap: 10, color: 'var(--text-muted)' },
  albums: { display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(110px, 1fr))', gap: 12 },
  album: { display: 'flex', flexDirection: 'column', gap: 6, padding: 0, border: 'none', background: 'transparent', color: 'var(--text)', cursor: 'pointer', textAlign: 'left', minWidth: 0, fontFamily: 'inherit' },
  artWrap: { aspectRatio: '1 / 1', borderRadius: 10, overflow: 'hidden', background: 'var(--surface)', display: 'block' },
  art: { width: '100%', height: '100%', objectFit: 'cover', display: 'block' },
  artFallback: { width: '100%', height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)' },
  albumTitle: { fontSize: 13, fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  tracks: { display: 'flex', flexDirection: 'column', gap: 4 },
  track: { display: 'flex', alignItems: 'center', gap: 8 },
  trackPlay: { width: 26, height: 26, borderRadius: '50%', border: '1px solid var(--border)', background: 'transparent', color: 'var(--text)', cursor: 'pointer', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
  trackText: { flex: 1, minWidth: 0 },
  trackTitle: { fontWeight: 600, fontSize: 14, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  trackMeta: { fontSize: 12, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
};

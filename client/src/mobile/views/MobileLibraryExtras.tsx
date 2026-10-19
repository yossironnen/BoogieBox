/**
 * Mobile Browse → Favourites and Radio: the signed-in user's hearts (songs, albums, artists) and the shared
 * internet radio stations. Mirrors the desktop Browse tabs (components/FavouritesView, RadioStationsView);
 * stations are edited on the desktop Settings/Browse surfaces by admins.
 */

import { useEffect, useState, type CSSProperties } from 'react';
import { api } from '../../api';
import ArtImage from '../../components/ArtImage';
import { HeartIcon, HeartToggle, RadioIcon, SongIcon } from '../../components/FavouriteIcons';
import { visibleFavourites } from '../../components/FavouritesView';
import { useFavourites, type Favourites } from '../../favourites';
import { hybridMobileContentStyles } from '../../hybridPreview';
import type { RadioStation } from '../../radioStations';
import type { Album, Artist, QueueSource, Track } from '../../types';

const EMPTY: Favourites = { artists: [], albums: [], tracks: [] };

/** Mobile Favourites list (songs first, then albums and artists). */
export function MobileFavouritesSection({
  onPlayTrack,
  onOpenAlbum,
  onOpenArtist,
  onTrackActions,
}: {
  onPlayTrack: (track: Track, allTracks?: Track[], source?: QueueSource) => void;
  onOpenAlbum: (album: Album) => void;
  onOpenArtist: (artist: Artist) => void;
  onTrackActions: (track: Track, allTracks: Track[]) => void;
}) {
  const favourites = useFavourites();
  const [data, setData] = useState<Favourites>(EMPTY);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  useEffect(() => {
    let cancelled = false;
    api.favourites.list()
      .then((result) => { if (!cancelled) setData(result); })
      .catch(() => { if (!cancelled) setError('Favourites could not be loaded. Try again in a moment.'); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, []);

  if (loading) return <div role="status" style={hybridMobileContentStyles.feedback}>Loading…</div>;
  if (error) return <div role="alert" style={{ ...hybridMobileContentStyles.feedback, ...hybridMobileContentStyles.feedbackError }}>{error}</div>;

  const shown = visibleFavourites(data, favourites.has);
  const source: QueueSource = { type: 'favourites', id: '0' };
  if (!shown.tracks.length && !shown.albums.length && !shown.artists.length) {
    return (
      <div role="status" style={{ ...hybridMobileContentStyles.feedback, ...styles.empty }}>
        <span style={{ color: 'var(--accent)', opacity: 0.85 }}><HeartIcon size={24} /></span>
        Add songs, albums and artists to Favourites from their actions.
      </div>
    );
  }

  return (
    <div style={{ display: 'grid', gap: 18 }}>
      {shown.tracks.length > 0 && (
        <section aria-label="Favourite songs" style={{ display: 'grid', gap: 8 }}>
          <h2 style={styles.heading}><span style={styles.headingIcon}><SongIcon size={15} /></span>Songs</h2>
          <div style={hybridMobileContentStyles.list}>
            {shown.tracks.map((track) => (
              <div key={String(track.id)} style={styles.row}>
                <button type="button" style={styles.rowPlay} onClick={() => onPlayTrack(track, shown.tracks, source)}>
                  <span style={hybridMobileContentStyles.listTitle}>{track.title || track.file_name}</span>
                  <span style={hybridMobileContentStyles.listSubtitle}>{track.artist || 'Unknown artist'}</span>
                </button>
                <HeartToggle
                  on={favourites.has('track', String(track.id))}
                  label={track.title || track.file_name}
                  onToggle={() => { void favourites.toggle('track', track.id).catch(() => {}); }}
                  size={18}
                />
                <button
                  type="button"
                  style={styles.more}
                  aria-label={`More actions for ${track.title || track.file_name}`}
                  onClick={() => onTrackActions(track, shown.tracks)}
                >
                  •••
                </button>
              </div>
            ))}
          </div>
        </section>
      )}

      {shown.albums.length > 0 && (
        <section aria-label="Favourite albums" style={{ display: 'grid', gap: 8 }}>
          <h2 style={styles.heading}><span style={styles.headingIcon}><AlbumGlyph /></span>Albums</h2>
          <div style={hybridMobileContentStyles.twoColumnGrid}>
            {shown.albums.map((album) => (
              <button key={String(album.id)} type="button" style={styles.card} onClick={() => onOpenAlbum(album)}>
                <span style={styles.cardArt}>
                  <ArtImage src={api.albumArtUrl(album.id, 300)} alt="" imgStyle={styles.cardImage} />
                </span>
                <span style={hybridMobileContentStyles.cardTitle}>{album.title}</span>
                <span style={hybridMobileContentStyles.cardMeta}>{album.album_artist || album.artist || ''}</span>
              </button>
            ))}
          </div>
        </section>
      )}

      {shown.artists.length > 0 && (
        <section aria-label="Favourite artists" style={{ display: 'grid', gap: 8 }}>
          <h2 style={styles.heading}><span style={styles.headingIcon}><ArtistGlyph /></span>Artists</h2>
          <div style={hybridMobileContentStyles.list}>
            {shown.artists.map((artist) => (
              <button key={String(artist.id)} type="button" style={hybridMobileContentStyles.listRow} onClick={() => onOpenArtist(artist)}>
                <span style={{ ...hybridMobileContentStyles.listArtwork, borderRadius: 16 }}>
                  <ArtImage src={api.artistPhotoUrl(artist.id, 300)} alt="" imgStyle={hybridMobileContentStyles.listArtworkImage} />
                </span>
                <span style={hybridMobileContentStyles.listMeta}>
                  <span style={hybridMobileContentStyles.listTitle}>{artist.name}</span>
                  <span style={hybridMobileContentStyles.listSubtitle}>{artist.album_count} albums • {artist.track_count} tracks</span>
                </span>
              </button>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

/** Mobile internet radio list. */
export function MobileRadioSection({
  onPlayStation,
  playingStationId = null,
}: {
  onPlayStation: (station: RadioStation) => void;
  playingStationId?: string | null;
}) {
  const [stations, setStations] = useState<RadioStation[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  useEffect(() => {
    let cancelled = false;
    api.radioStations.list()
      .then((list) => { if (!cancelled) setStations(list); })
      .catch(() => { if (!cancelled) setError('Radio stations could not be loaded. Try again in a moment.'); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, []);

  if (loading) return <div role="status" style={hybridMobileContentStyles.feedback}>Loading…</div>;
  if (error) return <div role="alert" style={{ ...hybridMobileContentStyles.feedback, ...hybridMobileContentStyles.feedbackError }}>{error}</div>;
  if (!stations.length) {
    return (
      <div role="status" style={{ ...hybridMobileContentStyles.feedback, ...styles.empty }}>
        <span style={{ color: 'var(--accent)', opacity: 0.85 }}><RadioIcon size={24} /></span>
        No radio stations yet.
      </div>
    );
  }
  return (
    <div aria-label="Radio stations" style={hybridMobileContentStyles.list}>
      {stations.map((station) => (
        <button
          key={station.id}
          type="button"
          style={{ ...hybridMobileContentStyles.listRow, ...(playingStationId === station.id ? styles.playing : {}) }}
          onClick={() => onPlayStation(station)}
        >
          <span style={{ ...hybridMobileContentStyles.listArtwork, ...styles.stationIcon }}><RadioIcon size={22} /></span>
          <span style={hybridMobileContentStyles.listMeta}>
            <span style={hybridMobileContentStyles.listTitle}>{station.name}</span>
            <span style={hybridMobileContentStyles.listSubtitle}>Internet radio</span>
          </span>
          <span aria-hidden="true" style={hybridMobileContentStyles.listBadge}>{playingStationId === station.id ? 'Playing' : 'Play'}</span>
        </button>
      ))}
    </div>
  );
}

const AlbumGlyph = () => (
  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
    <circle cx="12" cy="12" r="10" /><circle cx="12" cy="12" r="3" />
  </svg>
);
const ArtistGlyph = () => (
  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
    <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" /><circle cx="12" cy="7" r="4" />
  </svg>
);

const styles: Record<string, CSSProperties> = {
  empty: { display: 'flex', alignItems: 'center', gap: 10 },
  heading: { display: 'flex', alignItems: 'center', gap: 8, margin: 0, fontSize: 16, fontWeight: 700 },
  headingIcon: { color: 'var(--accent)', opacity: 0.85, display: 'inline-flex' },
  row: { display: 'flex', alignItems: 'center', gap: 6, padding: '6px 4px' },
  rowPlay: { flex: 1, minWidth: 0, display: 'grid', gap: 2, textAlign: 'left', border: 'none', background: 'transparent', color: 'var(--text)', padding: '4px 0', fontFamily: 'inherit', cursor: 'pointer' },
  more: { border: 'none', background: 'transparent', color: 'var(--text-muted)', fontSize: 16, padding: '6px 8px', cursor: 'pointer' },
  card: { display: 'grid', gap: 6, textAlign: 'left', border: 'none', background: 'transparent', color: 'var(--text)', padding: 0, fontFamily: 'inherit', cursor: 'pointer', minWidth: 0 },
  cardArt: { aspectRatio: '1 / 1', borderRadius: 14, overflow: 'hidden', background: 'var(--surface)', display: 'block' },
  cardImage: { width: '100%', height: '100%', objectFit: 'cover', display: 'block' },
  stationIcon: { display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--accent)' },
  playing: { outline: '1px solid color-mix(in srgb, var(--accent) 60%, transparent)' },
};

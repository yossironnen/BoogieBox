/**
 * Defines mobile Mobile Home View behavior for the BoogieBox React client.
 */

import { useEffect, useState, type ReactNode } from 'react';
import { api } from '../../api';
import ArtImage from '../../components/ArtImage';
import { hybridMobileContentStyles } from '../../hybridPreview';
import type { Album, ClientEntityId, HomeTopRated, LatestAlbum, Playlist, Track } from '../../types';
import { phase2 } from '../../uiPhase2';
import MobileSkeleton from '../components/MobileSkeleton';

type Props = {
  onOpenAlbum: (album: Album) => void;
  onOpenPlaylist: (playlistId: ClientEntityId) => void;
  onOpenBrowse: () => void;
  onPlayTrack: (track: Track, allTracks?: Track[]) => void;
};

function Section({
  title,
  onSeeAll,
  children,
}: {
  title: string;
  onSeeAll?: () => void;
  children: ReactNode;
}) {
  return (
    <section aria-labelledby={`mobile-home-${title.toLowerCase().replace(/\s+/g, '-')}`} style={hybridMobileContentStyles.section}>
      <div style={hybridMobileContentStyles.sectionHeader}>
        <h2
          id={`mobile-home-${title.toLowerCase().replace(/\s+/g, '-')}`}
          style={hybridMobileContentStyles.sectionTitle}
        >
          {title}
        </h2>
        {onSeeAll ? (
          <button
            type="button"
            style={hybridMobileContentStyles.sectionAction}
            onClick={onSeeAll}
          >
            See all
          </button>
        ) : null}
      </div>
      {children}
    </section>
  );
}

function ShelfArtwork({
  albumId,
  fallback = '♪',
}: {
  albumId?: ClientEntityId | null;
  fallback?: string;
}) {
  return (
    <span style={hybridMobileContentStyles.artworkFrame}>
      {albumId ? (
        <ArtImage
          src={api.albumArtUrl(albumId, 300)}
          alt=""
          imgStyle={hybridMobileContentStyles.artworkImage}
        />
      ) : (
        <span aria-hidden="true" style={hybridMobileContentStyles.artworkFallback}>
          {fallback}
        </span>
      )}
    </span>
  );
}

function RowArtwork({
  albumId,
  fallback = '♪',
}: {
  albumId?: ClientEntityId | null;
  fallback?: string;
}) {
  return (
    <span style={hybridMobileContentStyles.listArtwork}>
      {albumId ? (
        <ArtImage
          src={api.albumArtUrl(albumId, 300)}
          alt=""
          imgStyle={hybridMobileContentStyles.listArtworkImage}
        />
      ) : (
        <span aria-hidden="true" style={hybridMobileContentStyles.listArtworkFallback}>
          {fallback}
        </span>
      )}
    </span>
  );
}

/** Mobile Home View is part of this module's public API. */
export default function MobileHomeView({
  onOpenAlbum,
  onOpenPlaylist,
  onOpenBrowse,
  onPlayTrack,
}: Props) {
  const [albums, setAlbums] = useState<LatestAlbum[]>([]);
  const [topRated, setTopRated] = useState<HomeTopRated | null>(null);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [recentlyPlayed, setRecentlyPlayed] = useState<Track[]>([]);
  const [topPlayed, setTopPlayed] = useState<Track[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshKey, setRefreshKey] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    Promise.allSettled([
      api.latestAlbums(8),
      api.homeTopRated(5),
      api.playlists.list(),
      api.recentlyPlayed(10),
      api.topPlayedTracks(10),
    ]).then(results => {
      if (cancelled) return;
      if (results[0].status === 'fulfilled') setAlbums(results[0].value);
      if (results[1].status === 'fulfilled') setTopRated(results[1].value);
      if (results[2].status === 'fulfilled') setPlaylists(results[2].value.slice(0, 4));
      if (results[3].status === 'fulfilled') setRecentlyPlayed(results[3].value);
      if (results[4].status === 'fulfilled') setTopPlayed(results[4].value);
    }).finally(() => {
      if (!cancelled) setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [refreshKey]);

  const topRatedItems = topRated
    ? [...topRated.tracks.slice(0, 3), ...topRated.albums.slice(0, 2)]
    : [];
  const hasHomeContent = albums.length > 0
    || topRatedItems.length > 0
    || playlists.length > 0
    || recentlyPlayed.length > 0
    || topPlayed.length > 0;

  return (
    <main
      aria-busy={loading}
      style={{ ...phase2.mobilePage, display: 'grid', gap: 24 }}
    >
      <header style={hybridMobileContentStyles.pageHeader}>
        <div style={hybridMobileContentStyles.eyebrow}>Home</div>
        <h1 style={hybridMobileContentStyles.pageTitle}>What will you play next?</h1>
        <p style={hybridMobileContentStyles.pageBody}>
          Fresh additions, trusted favorites, and familiar rotations from your library.
        </p>
        <button
          type="button"
          disabled={loading}
          style={{
            ...hybridMobileContentStyles.secondaryAction,
            ...(loading ? hybridMobileContentStyles.disabled : {}),
          }}
          onClick={() => setRefreshKey(current => current + 1)}
        >
          Refresh
        </button>
      </header>

      {loading ? <MobileSkeleton variant="poster" count={6} /> : null}

      {!loading && albums.length > 0 ? (
        <Section title="Recently Added Music" onSeeAll={onOpenBrowse}>
          <div aria-label="Recently added albums" style={hybridMobileContentStyles.shelf}>
            {albums.map(album => (
              <button
                key={album.id}
                type="button"
                style={hybridMobileContentStyles.artworkCard}
                onClick={() => onOpenAlbum(album)}
              >
                <ShelfArtwork albumId={album.id} />
                <span style={hybridMobileContentStyles.cardTitle}>{album.title}</span>
                <span style={hybridMobileContentStyles.cardMeta}>
                  {album.album_artist || album.artist || 'Unknown artist'}
                </span>
              </button>
            ))}
          </div>
        </Section>
      ) : null}

      {!loading && topRated && topRatedItems.length > 0 ? (
        <Section title="Top Rated">
          <div style={hybridMobileContentStyles.list}>
            {topRatedItems.map(item => (
              'file_name' in item ? (
                <button
                  key={`track-${item.id}`}
                  type="button"
                  style={hybridMobileContentStyles.listRow}
                  onClick={() => onPlayTrack(item, topRated.tracks)}
                >
                  <RowArtwork albumId={item.album_id} />
                  <span style={hybridMobileContentStyles.listMeta}>
                    <span style={hybridMobileContentStyles.listTitle}>
                      {item.title || item.file_name}
                    </span>
                    <span style={hybridMobileContentStyles.listSubtitle}>
                      {item.artist || 'Unknown artist'}
                    </span>
                  </span>
                  <span style={hybridMobileContentStyles.listBadge}>
                    {item.rating ? `${item.rating} ★` : 'Track'}
                  </span>
                </button>
              ) : (
                <button
                  key={`album-${item.id}`}
                  type="button"
                  style={hybridMobileContentStyles.listRow}
                  onClick={() => onOpenAlbum(item)}
                >
                  <RowArtwork albumId={item.id} />
                  <span style={hybridMobileContentStyles.listMeta}>
                    <span style={hybridMobileContentStyles.listTitle}>{item.title}</span>
                    <span style={hybridMobileContentStyles.listSubtitle}>
                      {item.album_artist || item.artist || 'Unknown artist'}
                    </span>
                  </span>
                  <span style={hybridMobileContentStyles.listBadge}>
                    {item.rating ? `${item.rating} ★` : 'Album'}
                  </span>
                </button>
              )
            ))}
          </div>
        </Section>
      ) : null}

      {!loading && playlists.length > 0 ? (
        <Section title="Your Playlists">
          <div style={hybridMobileContentStyles.list}>
            {playlists.map(playlist => (
              <button
                key={playlist.id}
                type="button"
                style={hybridMobileContentStyles.listRow}
                onClick={() => onOpenPlaylist(playlist.id)}
              >
                <RowArtwork albumId={playlist.art_album_ids?.[0]} fallback="≡" />
                <span style={hybridMobileContentStyles.listMeta}>
                  <span style={hybridMobileContentStyles.listTitle}>{playlist.name}</span>
                  <span style={hybridMobileContentStyles.listSubtitle}>
                    Ready when you are
                  </span>
                </span>
                <span style={hybridMobileContentStyles.listBadge}>
                  {playlist.track_count ?? 0} tracks
                </span>
              </button>
            ))}
          </div>
        </Section>
      ) : null}

      {!loading && recentlyPlayed.length > 0 ? (
        <Section title="Recently Played">
          <div aria-label="Recently played tracks" style={hybridMobileContentStyles.shelf}>
            {recentlyPlayed.map(track => (
              <button
                key={track.id}
                type="button"
                style={hybridMobileContentStyles.artworkCard}
                onClick={() => onPlayTrack(track, recentlyPlayed)}
              >
                <ShelfArtwork albumId={track.album_id} />
                <span style={hybridMobileContentStyles.cardTitle}>
                  {track.title || track.file_name}
                </span>
                <span style={hybridMobileContentStyles.cardMeta}>
                  {track.artist || 'Unknown artist'}
                </span>
              </button>
            ))}
          </div>
        </Section>
      ) : null}

      {!loading && topPlayed.length > 0 ? (
        <Section title="Top Played Tracks">
          <div style={hybridMobileContentStyles.list}>
            {topPlayed.slice(0, 10).map(track => (
              <button
                key={track.id}
                type="button"
                style={hybridMobileContentStyles.listRow}
                onClick={() => onPlayTrack(track, topPlayed)}
              >
                <RowArtwork albumId={track.album_id} />
                <span style={hybridMobileContentStyles.listMeta}>
                  <span style={hybridMobileContentStyles.listTitle}>
                    {track.title || track.file_name}
                  </span>
                  <span style={hybridMobileContentStyles.listSubtitle}>
                    {track.artist || 'Unknown artist'}
                  </span>
                </span>
                <span style={hybridMobileContentStyles.listBadge}>Play</span>
              </button>
            ))}
          </div>
        </Section>
      ) : null}

      {!loading && !hasHomeContent ? (
        <div role="status" style={hybridMobileContentStyles.empty}>
          <div style={hybridMobileContentStyles.emptyTitle}>Home is quiet right now</div>
          <p style={hybridMobileContentStyles.emptyBody}>
            Add music or refresh after your library finishes scanning.
          </p>
        </div>
      ) : null}
    </main>
  );
}

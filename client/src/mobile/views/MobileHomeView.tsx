/**
 * Defines mobile Mobile Home View behavior for the BoogieBox React client.
 */

import React, { useEffect, useState } from 'react';
import { api } from '../../api';
import type { Album, ClientEntityId, HomeTopRated, LatestAlbum, Playlist, Track } from '../../types';
import ArtImage from '../../components/ArtImage';
import MobileSkeleton from '../components/MobileSkeleton';
import { phase2 } from '../../uiPhase2';

type Props = {
  onOpenAlbum: (album: Album) => void;
  onOpenPlaylist: (playlistId: ClientEntityId) => void;
  onOpenBrowse: () => void;
  onPlayTrack: (track: Track, allTracks?: Track[]) => void;
};

function sectionHasItems<T>(items: T[]) {
  return items.length > 0;
}

function Section({ title, onSeeAll, children }: { title: string; onSeeAll?: () => void; children: React.ReactNode }) {
  return (
    <section style={styles.section}>
      <div style={styles.sectionHeader}>
        <h2 style={styles.sectionTitle}>{title}</h2>
        {onSeeAll ? <button type="button" style={styles.seeAll} onClick={onSeeAll}>See all</button> : null}
      </div>
      {children}
    </section>
  );
}

/** Mobile Home View is part of this module's public API. */
export default function MobileHomeView({ onOpenAlbum, onOpenPlaylist, onOpenBrowse, onPlayTrack }: Props) {
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
    ]).then((results) => {
      if (cancelled) return;
      if (results[0].status === 'fulfilled') setAlbums(results[0].value);
      if (results[1].status === 'fulfilled') setTopRated(results[1].value);
      if (results[2].status === 'fulfilled') setPlaylists(results[2].value.slice(0, 4));
      if (results[3].status === 'fulfilled') setRecentlyPlayed(results[3].value);
      if (results[4].status === 'fulfilled') setTopPlayed(results[4].value);
    }).finally(() => {
      if (!cancelled) setLoading(false);
    });
    return () => { cancelled = true; };
  }, [refreshKey]);

  return (
    <div style={styles.page}>
      <div style={styles.hero}>
        <div style={styles.kicker}>Home</div>
        <h1 style={styles.title}>Listen, rediscover, and pick your next play.</h1>
        <button type="button" style={styles.refresh} onClick={() => setRefreshKey((current) => current + 1)}>
          Refresh
        </button>
      </div>

      {loading ? <MobileSkeleton variant="poster" count={6} /> : null}

      {!loading && sectionHasItems(albums) ? (
        <Section title="Recently Added Music" onSeeAll={onOpenBrowse}>
          <div style={styles.squareRow}>
            {albums.map((album) => (
              <button key={album.id} type="button" style={styles.squareCard} onClick={() => onOpenAlbum(album)}>
                <ArtImage src={api.albumArtUrl(album.id, 300)} alt="" imgStyle={styles.squareImage} />
                <span style={styles.cardTitle}>{album.title}</span>
                <span style={styles.cardSub}>{album.album_artist || album.artist || 'Unknown artist'}</span>
              </button>
            ))}
          </div>
        </Section>
      ) : null}

      {!loading && topRated && (topRated.artists.length || topRated.albums.length || topRated.tracks.length) ? (
        <Section title="Top Rated">
          <div style={styles.list}>
            {[...topRated.tracks.slice(0, 3), ...topRated.albums.slice(0, 2)].map((item) => (
              'file_name' in item ? (
                <button key={`track-${item.id}`} type="button" style={styles.row} onClick={() => onPlayTrack(item, topRated.tracks)}>
                  <span style={styles.rowTitle}>{item.title || item.file_name}</span>
                  <span style={styles.rowSub}>{item.artist || 'Unknown artist'}</span>
                </button>
              ) : (
                <button key={`album-${item.id}`} type="button" style={styles.row} onClick={() => onOpenAlbum(item)}>
                  <span style={styles.rowTitle}>{item.title}</span>
                  <span style={styles.rowSub}>{item.album_artist || item.artist || 'Unknown artist'}</span>
                </button>
              )
            ))}
          </div>
        </Section>
      ) : null}

      {!loading && sectionHasItems(playlists) ? (
        <Section title="Your Playlists">
          <div style={styles.list}>
            {playlists.map((playlist) => (
              <button key={playlist.id} type="button" style={styles.row} onClick={() => onOpenPlaylist(playlist.id)}>
                <span style={styles.rowTitle}>{playlist.name}</span>
                <span style={styles.rowSub}>{playlist.track_count ?? 0} tracks</span>
              </button>
            ))}
          </div>
        </Section>
      ) : null}

      {!loading && sectionHasItems(recentlyPlayed) ? (
        <Section title="Recently Played">
          <div style={styles.squareRow}>
            {recentlyPlayed.map((track) => (
              <button key={track.id} type="button" style={styles.squareCard} onClick={() => onPlayTrack(track, recentlyPlayed)}>
                {track.album_id ? <ArtImage src={api.albumArtUrl(track.album_id, 300)} alt="" imgStyle={styles.squareImage} /> : <span style={styles.squareFallback} />}
                <span style={styles.cardTitle}>{track.title || track.file_name}</span>
                <span style={styles.cardSub}>{track.artist || 'Unknown artist'}</span>
              </button>
            ))}
          </div>
        </Section>
      ) : null}

      {!loading && sectionHasItems(topPlayed) ? (
        <Section title="Top Played Tracks">
          <div style={styles.list}>
            {topPlayed.slice(0, 10).map((track) => (
              <button key={track.id} type="button" style={styles.row} onClick={() => onPlayTrack(track, topPlayed)}>
                <span style={styles.rowTitle}>{track.title || track.file_name}</span>
                <span style={styles.rowSub}>{track.artist || 'Unknown artist'}</span>
              </button>
            ))}
          </div>
        </Section>
      ) : null}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  page: { ...phase2.mobilePage, display: 'grid', gap: 22 },
  hero: { display: 'grid', gap: 10 },
  kicker: phase2.mobileKicker,
  title: { ...phase2.mobileTitle, margin: 0, maxWidth: 330 },
  refresh: { justifySelf: 'start', minHeight: 44, padding: '0 16px', borderRadius: 999, border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text)', fontFamily: 'inherit', fontWeight: 700 },
  section: { display: 'grid', gap: 12 },
  sectionHeader: { display: 'flex', alignItems: 'center', justifyContent: 'space-between' },
  sectionTitle: { margin: 0, color: 'var(--text)', fontSize: 18, fontWeight: 800 },
  seeAll: { minHeight: 38, padding: '0 12px', borderRadius: 999, border: '1px solid var(--border)', background: 'transparent', color: 'var(--text)', fontFamily: 'inherit', fontSize: 12, fontWeight: 700 },
  squareRow: { display: 'flex', gap: 12, overflowX: 'auto', paddingBottom: 4 },
  squareCard: { flex: '0 0 132px', display: 'grid', gap: 5, padding: 0, border: 'none', background: 'transparent', color: 'var(--text)', textAlign: 'left', fontFamily: 'inherit' },
  squareImage: { width: 132, height: 132, borderRadius: 10, objectFit: 'cover', display: 'block' },
  squareFallback: { width: 132, height: 132, borderRadius: 10, background: 'var(--surface)', border: '1px solid var(--border)' },
  cardTitle: { minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: 'var(--text)', fontSize: 12, fontWeight: 800 },
  cardSub: { color: 'var(--text-muted)', fontSize: 11, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  list: { display: 'grid', gap: 8 },
  row: { display: 'grid', gap: 4, minHeight: 58, padding: '10px 14px', borderRadius: 14, border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text)', textAlign: 'left', fontFamily: 'inherit' },
  rowTitle: { fontSize: 14, fontWeight: 800, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  rowSub: { fontSize: 12, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
};

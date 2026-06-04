/**
 * Defines mobile Mobile Browse View behavior for the BoogieBox React client.
 */

import React, { useEffect, useState } from 'react';
import { api } from '../../api';
import type { Album, Artist, Library, Playlist, Track } from '../../types';
import ArtImage from '../../components/ArtImage';
import StarRating from '../../components/StarRating';
import type { PlaybackSnapshot } from '../../components/Player';
import type { MobileBrowseSelection } from '../mobileShell';
import { useMobileTrackActions } from '../components/MobileActionSheets';

/** Clear Mobile Artist Browse Cache is part of this module's public API. */
export function clearMobileArtistBrowseCache() {}

/** Mobile Browse View is part of this module's public API. */
export default function MobileBrowseView({
  onPlayTrack,
  onAddToQueue,
  selection,
  onSelectionChange,
  playbackSnapshot,
}: {
  onPlayTrack: (track: Track, allTracks?: Track[]) => void;
  onAddToQueue: (track: Track) => void;
  selection: MobileBrowseSelection;
  onSelectionChange: (selection: MobileBrowseSelection) => void;
  playbackSnapshot?: PlaybackSnapshot | null;
  libraries?: Library[];
}) {
  const [artists, setArtists] = useState<Artist[]>([]);
  const [albums, setAlbums] = useState<Album[]>([]);
  const [tracks, setTracks] = useState<Track[]>(selection.tracks);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    api.playlists.list().then(setPlaylists).catch(() => {});
  }, []);

  useEffect(() => {
    if (selection.artist || selection.album) return;
    setLoading(true);
    api.artists().then(setArtists).finally(() => setLoading(false));
  }, [selection.artist, selection.album]);

  useEffect(() => {
    if (!selection.artist) {
      setAlbums([]);
      return;
    }
    setLoading(true);
    api.artistAlbums(selection.artist.id).then(setAlbums).finally(() => setLoading(false));
  }, [selection.artist]);

  useEffect(() => {
    if (!selection.album) {
      setTracks([]);
      return;
    }
    setLoading(true);
    api.albumTracks(selection.album.id).then((nextTracks) => {
      setTracks(nextTracks);
      onSelectionChange({ artist: selection.artist, album: selection.album, tracks: nextTracks });
    }).finally(() => setLoading(false));
  }, [onSelectionChange, selection.album, selection.artist]);

  const trackActions = useMobileTrackActions<Track>({ playlists, onPlayTrack, onAddToQueue });
  const nowPlayingId = playbackSnapshot?.currentTrack?.id ?? null;

  return (
    <div style={styles.page}>
      <header style={styles.header}>
        {(selection.artist || selection.album) ? (
          <button
            type="button"
            style={styles.backBtn}
            onClick={() => {
              if (selection.album) onSelectionChange({ ...selection, album: null, tracks: [] });
              else onSelectionChange({ artist: null, album: null, tracks: [] });
            }}
          >
            Back
          </button>
        ) : null}
        <div style={styles.eyebrow}>{selection.album ? 'Tracks' : selection.artist ? 'Albums' : 'Artists'}</div>
        <div style={styles.title}>{selection.album?.title ?? selection.artist?.name ?? 'Browse'}</div>
      </header>

      {loading ? <div style={styles.emptyState}>Loading...</div> : null}

      {!selection.artist && !selection.album ? (
        <div style={styles.list}>
          {artists.map((artist) => (
            <button key={artist.id} type="button" style={styles.row} onClick={() => onSelectionChange({ artist, album: null, tracks: [] })}>
              <span>{artist.name}</span>
              <span style={styles.meta}>{artist.album_count} albums * {artist.track_count} tracks</span>
            </button>
          ))}
        </div>
      ) : null}

      {selection.artist && !selection.album ? (
        <div style={styles.grid}>
          {albums.map((album) => (
            <button key={album.id} type="button" style={styles.card} onClick={() => onSelectionChange({ artist: selection.artist, album, tracks: [] })}>
              <ArtImage src={api.albumArtUrl(album.id, 300)} alt="" imgStyle={styles.cover} />
              <span style={styles.cardTitle}>{album.title}</span>
              <span style={styles.meta}>{album.year ?? 'Unknown year'} * {album.track_count} tracks</span>
            </button>
          ))}
        </div>
      ) : null}

      {selection.album ? (
        <div style={styles.list}>
          {tracks.map((track, index) => {
            const isPlaying = nowPlayingId === track.id;
            return (
              <div key={track.id} style={styles.trackRow}>
                <button type="button" style={styles.trackMain} onClick={() => onPlayTrack(track, tracks)}>
                  <span style={styles.trackIndex}>{index + 1}</span>
                  <span style={styles.trackMeta}>
                    <span style={isPlaying ? styles.playingText : undefined}>{track.title || track.file_name}</span>
                    <span style={styles.meta}>{track.artist || selection.artist?.name || 'Unknown artist'}</span>
                    <span onClick={(event) => event.stopPropagation()}>
                      <StarRating
                        value={track.rating ?? null}
                        onChange={async (rating) => {
                          setTracks((current) => current.map((candidate) => candidate.id === track.id ? { ...candidate, rating } : candidate));
                          await api.setTrackRating(track.id, rating);
                        }}
                        ariaLabel={`Rate ${track.title || track.file_name}`}
                        size="compact"
                      />
                    </span>
                  </span>
                </button>
                <button type="button" style={styles.queueBtn} onClick={() => trackActions.openForTrack(track, tracks)} aria-label={`More actions for ${track.title || track.file_name}`}>
                  ...
                </button>
              </div>
            );
          })}
        </div>
      ) : null}
      {trackActions.actionsSheet}
      {trackActions.pickerSheet}
      {trackActions.createSheet}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  page: { padding: '20px 16px 180px' },
  header: { display: 'grid', gap: 8, marginBottom: 18 },
  eyebrow: { color: 'var(--text-muted)', fontSize: 12, textTransform: 'uppercase', letterSpacing: 1, fontWeight: 800 },
  title: { color: 'var(--text)', fontSize: 28, fontWeight: 900 },
  backBtn: { justifySelf: 'start', minHeight: 40, padding: '0 14px', borderRadius: 999, border: '1px solid var(--border)', background: 'transparent', color: 'var(--text)', fontFamily: 'inherit' },
  list: { display: 'grid', gap: 10 },
  row: { display: 'grid', gap: 4, minHeight: 58, padding: '12px 14px', borderRadius: 14, border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text)', textAlign: 'left', fontFamily: 'inherit' },
  meta: { color: 'var(--text-muted)', fontSize: 12 },
  grid: { display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 14 },
  card: { display: 'grid', gap: 8, padding: 0, border: 'none', background: 'transparent', color: 'var(--text)', textAlign: 'left', fontFamily: 'inherit' },
  cover: { width: '100%', aspectRatio: '1 / 1', borderRadius: 12, objectFit: 'cover', display: 'block' },
  cardTitle: { fontSize: 14, fontWeight: 800, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  trackRow: { display: 'flex', alignItems: 'stretch', gap: 10 },
  trackMain: { flex: 1, display: 'flex', alignItems: 'center', gap: 12, minHeight: 68, padding: '0 14px', borderRadius: 20, border: '1px solid var(--border)', background: 'rgba(255,255,255,0.02)', color: 'var(--text)', textAlign: 'left' },
  trackIndex: { color: 'var(--text-muted)', minWidth: 16, fontSize: 13 },
  trackMeta: { minWidth: 0, display: 'grid', gap: 4, flex: 1 },
  queueBtn: { minWidth: 56, borderRadius: 18, border: '1px solid var(--border)', background: 'rgba(255,255,255,0.02)', color: 'var(--text)', fontSize: 18, fontFamily: 'inherit', letterSpacing: 1 },
  playingText: { color: 'var(--accent)' },
  emptyState: { color: 'var(--text-muted)', padding: '8px 0 2px' },
};

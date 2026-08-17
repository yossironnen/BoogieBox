/**
 * Defines mobile Mobile Browse View behavior for the BoogieBox React client.
 */

import { useEffect, useState, type CSSProperties } from 'react';
import { api } from '../../api';
import ArtImage from '../../components/ArtImage';
import type { PlaybackSnapshot } from '../../components/Player';
import StarRating from '../../components/StarRating';
import { hybridMobileContentStyles } from '../../hybridPreview';
import type { Album, Artist, Library, Playlist, SimilarArtist, Track } from '../../types';
import { phase2 } from '../../uiPhase2';
import { useMobileTrackActions } from '../components/MobileActionSheets';
import type { MobileBrowseSelection } from '../mobileShell';

/** Clear Mobile Artist Browse Cache is part of this module's public API. */
export function clearMobileArtistBrowseCache() {}

/** Mobile Browse View is part of this module's public API. */
export default function MobileBrowseView({
  onPlayTrack,
  onAddToQueue,
  selection,
  onSelectionChange,
  playbackSnapshot,
  hideCompilationOnlyArtists = true,
}: {
  onPlayTrack: (track: Track, allTracks?: Track[]) => void;
  onAddToQueue: (track: Track) => void;
  selection: MobileBrowseSelection;
  onSelectionChange: (selection: MobileBrowseSelection) => void;
  playbackSnapshot?: PlaybackSnapshot | null;
  libraries?: Library[];
  hideCompilationOnlyArtists?: boolean;
}) {
  const [artists, setArtists] = useState<Artist[]>([]);
  const [albums, setAlbums] = useState<Album[]>([]);
  const [tracks, setTracks] = useState<Track[]>(selection.tracks);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [similarArtists, setSimilarArtists] = useState<SimilarArtist[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    api.playlists.list().then(setPlaylists).catch(() => {});
  }, []);

  useEffect(() => {
    if (selection.artist || selection.album) return;
    let cancelled = false;
    setLoading(true);
    setError('');
    api.artists({ hide_compilation_only: hideCompilationOnlyArtists })
      .then(nextArtists => {
        if (!cancelled) setArtists(nextArtists);
      })
      .catch(() => {
        if (!cancelled) {
          setArtists([]);
          setError('Artists could not be loaded. Try again in a moment.');
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selection.artist, selection.album, hideCompilationOnlyArtists]);

  useEffect(() => {
    if (!selection.artist) {
      setAlbums([]);
      return;
    }
    if (selection.album) return;
    let cancelled = false;
    setLoading(true);
    setError('');
    api.artistAlbums(selection.artist.id)
      .then(nextAlbums => {
        if (!cancelled) setAlbums(nextAlbums);
      })
      .catch(() => {
        if (!cancelled) {
          setAlbums([]);
          setError('Albums could not be loaded. Try again in a moment.');
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selection.album, selection.artist]);

  useEffect(() => {
    if (!selection.album) {
      setTracks([]);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError('');
    api.albumTracks(selection.album.id)
      .then(nextTracks => {
        if (cancelled) return;
        setTracks(nextTracks);
        onSelectionChange({
          artist: selection.artist,
          album: selection.album,
          tracks: nextTracks,
        });
      })
      .catch(() => {
        if (!cancelled) {
          setTracks([]);
          setError('Tracks could not be loaded. Try again in a moment.');
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [onSelectionChange, selection.album, selection.artist]);

  useEffect(() => {
    if (!selection.artist || selection.album) {
      setSimilarArtists([]);
      return;
    }
    let cancelled = false;
    setSimilarArtists([]);
    api.artistSimilar(selection.artist.id, 12)
      .then(response => {
        if (!cancelled) setSimilarArtists(response.artists);
      })
      .catch(() => {
        if (!cancelled) setSimilarArtists([]);
      });
    return () => {
      cancelled = true;
    };
  }, [selection.album, selection.artist]);

  const trackActions = useMobileTrackActions<Track>({ playlists, onPlayTrack, onAddToQueue });
  const nowPlayingId = playbackSnapshot?.currentTrack?.id ?? null;
  const atRoot = !selection.artist && !selection.album;
  const atArtist = Boolean(selection.artist && !selection.album);

  return (
    <main aria-busy={loading} style={{ ...phase2.mobilePage, display: 'grid', gap: 18 }}>
      <header style={hybridMobileContentStyles.pageHeader}>
        {!atRoot ? (
          <button
            type="button"
            aria-label="Back"
            style={hybridMobileContentStyles.secondaryAction}
            onClick={() => {
              if (selection.album) {
                onSelectionChange({ ...selection, album: null, tracks: [] });
              } else {
                onSelectionChange({ artist: null, album: null, tracks: [] });
              }
            }}
          >
            ← Back
          </button>
        ) : null}

        {atRoot ? (
          <>
            <div style={hybridMobileContentStyles.eyebrow}>Browse</div>
            <h1 style={hybridMobileContentStyles.pageTitle}>Artists</h1>
            <p style={hybridMobileContentStyles.pageBody}>
              Move from an artist to a release, then start anywhere in the album.
            </p>
          </>
        ) : (
          <div style={hybridMobileContentStyles.detailHero}>
            <span
              style={{
                ...hybridMobileContentStyles.detailArtwork,
                ...(atArtist ? { borderRadius: 24 } : {}),
              }}
            >
              {atArtist && selection.artist ? (
                <ArtImage
                  src={api.artistPhotoUrl(selection.artist.id, 300)}
                  alt=""
                  imgStyle={styles.detailImage}
                />
              ) : selection.album ? (
                <ArtImage
                  src={api.albumArtUrl(selection.album.id, 300)}
                  alt=""
                  imgStyle={styles.detailImage}
                />
              ) : null}
            </span>
            <div style={{ minWidth: 0 }}>
              <div style={hybridMobileContentStyles.eyebrow}>
                {selection.album ? 'Album' : 'Artist'}
              </div>
              <h1 style={hybridMobileContentStyles.detailTitle}>
                {selection.album?.title ?? selection.artist?.name}
              </h1>
              <div style={hybridMobileContentStyles.detailMeta}>
                {selection.album
                  ? `${selection.album.year ?? 'Unknown year'} • ${selection.album.track_count} tracks`
                  : `${selection.artist?.album_count ?? 0} albums • ${selection.artist?.track_count ?? 0} tracks`}
              </div>
            </div>
          </div>
        )}
      </header>

      {loading ? (
        <div role="status" style={hybridMobileContentStyles.feedback}>Loading…</div>
      ) : null}
      {error ? (
        <div
          role="alert"
          style={{
            ...hybridMobileContentStyles.feedback,
            ...hybridMobileContentStyles.feedbackError,
          }}
        >
          {error}
        </div>
      ) : null}

      {atRoot && !loading && !error ? (
        artists.length > 0 ? (
          <div aria-label="Artists" style={hybridMobileContentStyles.list}>
            {artists.map(artist => (
              <button
                key={artist.id}
                type="button"
                style={hybridMobileContentStyles.listRow}
                onClick={() => onSelectionChange({ artist, album: null, tracks: [] })}
              >
                <span style={{ ...hybridMobileContentStyles.listArtwork, borderRadius: 16 }}>
                  <ArtImage
                    src={api.artistPhotoUrl(artist.id, 300)}
                    alt=""
                    imgStyle={hybridMobileContentStyles.listArtworkImage}
                  />
                </span>
                <span style={hybridMobileContentStyles.listMeta}>
                  <span style={hybridMobileContentStyles.listTitle}>{artist.name}</span>
                  <span style={hybridMobileContentStyles.listSubtitle}>
                    {artist.album_count} albums • {artist.track_count} tracks
                  </span>
                </span>
                <span aria-hidden="true" style={hybridMobileContentStyles.listBadge}>Open</span>
              </button>
            ))}
          </div>
        ) : (
          <div role="status" style={hybridMobileContentStyles.feedback}>
            No artists are available yet.
          </div>
        )
      ) : null}

      {atArtist && !loading && !error ? (
        albums.length > 0 ? (
          <div aria-label="Albums" style={hybridMobileContentStyles.twoColumnGrid}>
            {albums.map(album => (
              <button
                key={album.id}
                type="button"
                style={styles.albumCard}
                onClick={() => onSelectionChange({ artist: selection.artist, album, tracks: [] })}
              >
                <span style={styles.albumArtwork}>
                  <ArtImage
                    src={api.albumArtUrl(album.id, 300)}
                    alt=""
                    imgStyle={styles.detailImage}
                  />
                </span>
                <span style={hybridMobileContentStyles.cardTitle}>{album.title}</span>
                <span style={hybridMobileContentStyles.cardMeta}>
                  {album.year ?? 'Unknown year'} • {album.track_count} tracks
                </span>
              </button>
            ))}
          </div>
        ) : (
          <div role="status" style={hybridMobileContentStyles.feedback}>
            No albums are available for this artist.
          </div>
        )
      ) : null}

      {atArtist && similarArtists.length > 0 ? (
        <section style={styles.similarSection}>
          <h2 style={styles.similarHeading}>similar artists</h2>
          <div style={styles.similarGrid}>
            {similarArtists.map(artist => (
              <button
                key={artist.id}
                type="button"
                style={styles.similarCard}
                onClick={() => onSelectionChange({ artist, album: null, tracks: [] })}
              >
                <span style={styles.similarArtwork}>
                  <ArtImage
                    src={api.artistPhotoUrl(artist.id, 300)}
                    alt=""
                    imgStyle={styles.detailImage}
                  />
                </span>
                <span style={styles.similarName}>{artist.name}</span>
              </button>
            ))}
          </div>
        </section>
      ) : null}

      {selection.album && !loading && !error ? (
        tracks.length > 0 ? (
          <div aria-label="Album tracks" style={hybridMobileContentStyles.list}>
            {tracks.map((track, index) => {
              const isPlaying = nowPlayingId === track.id;
              return (
                <div
                  key={track.id}
                  style={{
                    ...styles.trackRow,
                    ...(isPlaying ? styles.trackRowPlaying : {}),
                  }}
                >
                  <span
                    aria-hidden="true"
                    style={{
                      ...styles.trackIndex,
                      ...(isPlaying ? styles.trackIndexPlaying : {}),
                    }}
                  >
                    {isPlaying ? '▶' : index + 1}
                  </span>
                  <div style={styles.trackContent}>
                    <button
                      type="button"
                      style={styles.trackPlay}
                      onClick={() => onPlayTrack(track, tracks)}
                    >
                      <span style={isPlaying ? styles.playingText : styles.trackTitle}>
                        {track.title || track.file_name}
                      </span>
                      <span style={hybridMobileContentStyles.listSubtitle}>
                        {track.artist || selection.artist?.name || 'Unknown artist'}
                      </span>
                    </button>
                    <StarRating
                      value={track.rating ?? null}
                      onChange={async rating => {
                        setTracks(current => current.map(candidate => (
                          candidate.id === track.id ? { ...candidate, rating } : candidate
                        )));
                        await api.setTrackRating(track.id, rating);
                      }}
                      ariaLabel={`Rate ${track.title || track.file_name}`}
                      size="compact"
                    />
                  </div>
                  <button
                    type="button"
                    style={styles.queueButton}
                    onClick={() => trackActions.openForTrack(track, tracks)}
                    aria-label={`More actions for ${track.title || track.file_name}`}
                  >
                    •••
                  </button>
                </div>
              );
            })}
          </div>
        ) : (
          <div role="status" style={hybridMobileContentStyles.feedback}>
            No tracks are available for this album.
          </div>
        )
      ) : null}

      {trackActions.actionsSheet}
      {trackActions.pickerSheet}
      {trackActions.createSheet}
    </main>
  );
}

const styles: Record<string, CSSProperties> = {
  detailImage: {
    width: '100%',
    height: '100%',
    display: 'block',
    objectFit: 'cover',
  },
  albumCard: {
    minWidth: 0,
    display: 'grid',
    alignContent: 'start',
    gap: 5,
    padding: 0,
    border: 'none',
    background: 'transparent',
    color: 'var(--text)',
    cursor: 'pointer',
    fontFamily: 'inherit',
    textAlign: 'left',
  },
  albumArtwork: {
    width: '100%',
    aspectRatio: '1 / 1',
    marginBottom: 3,
    overflow: 'hidden',
    borderRadius: 14,
    background: 'var(--surface-subtle)',
    boxShadow: 'var(--shadow-subtle)',
  },
  similarSection: {
    display: 'grid',
    gap: 10,
    paddingTop: 4,
  },
  similarHeading: {
    margin: 0,
    color: 'var(--text-muted)',
    fontSize: 12,
    fontWeight: 800,
    letterSpacing: '0.08em',
    textTransform: 'uppercase',
  },
  similarGrid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
    gap: 12,
  },
  similarCard: {
    minWidth: 0,
    minHeight: 44,
    display: 'grid',
    justifyItems: 'center',
    gap: 8,
    padding: 10,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 16,
    background: 'var(--surface)',
    color: 'var(--text)',
    cursor: 'pointer',
    fontFamily: 'inherit',
  },
  similarArtwork: {
    width: '100%',
    maxWidth: 128,
    aspectRatio: '1 / 1',
    overflow: 'hidden',
    borderRadius: '50%',
    background: 'var(--surface-subtle)',
    boxShadow: 'var(--shadow-subtle)',
  },
  similarName: {
    maxWidth: '100%',
    overflow: 'hidden',
    fontSize: 13,
    fontWeight: 750,
    lineHeight: 1.25,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  trackRow: {
    minHeight: 76,
    display: 'grid',
    gridTemplateColumns: '30px minmax(0, 1fr) 44px',
    alignItems: 'center',
    gap: 6,
    boxSizing: 'border-box',
    padding: '7px 7px 7px 9px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface)',
  },
  trackRowPlaying: {
    borderColor: 'color-mix(in srgb, var(--accent) 34%, var(--divider-subtle))',
    background: 'var(--accent-soft)',
  },
  trackIndex: {
    width: 24,
    height: 24,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 8,
    color: 'var(--text-faint)',
    fontSize: 11,
    fontWeight: 700,
  },
  trackIndexPlaying: {
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    fontSize: 9,
  },
  trackContent: {
    minWidth: 0,
    display: 'grid',
    gap: 4,
    padding: '3px 0',
  },
  trackPlay: {
    minWidth: 0,
    minHeight: 36,
    display: 'grid',
    alignContent: 'center',
    gap: 2,
    padding: 0,
    border: 'none',
    background: 'transparent',
    color: 'var(--text)',
    cursor: 'pointer',
    fontFamily: 'inherit',
    textAlign: 'left',
  },
  trackTitle: {
    overflow: 'hidden',
    color: 'var(--text)',
    fontSize: 13,
    fontWeight: 750,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  playingText: {
    overflow: 'hidden',
    color: 'var(--accent)',
    fontSize: 13,
    fontWeight: 800,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  queueButton: {
    width: 44,
    minWidth: 44,
    height: 44,
    padding: 0,
    border: 'none',
    borderRadius: 12,
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    cursor: 'pointer',
    fontFamily: 'inherit',
    fontSize: 13,
    letterSpacing: 1,
  },
};

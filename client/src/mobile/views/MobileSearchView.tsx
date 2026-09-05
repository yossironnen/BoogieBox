/**
 * Defines mobile Mobile Search View behavior for the BoogieBox React client.
 */

import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { api } from '../../api';
import ArtImage from '../../components/ArtImage';
import StarRating from '../../components/StarRating';
import { hybridMobileContentStyles } from '../../hybridPreview';
import type { Playlist, SearchResult, Track } from '../../types';
import { phase2 } from '../../uiPhase2';
import { useMobileTrackActions } from '../components/MobileActionSheets';
import MobileBottomSheet from '../components/MobileBottomSheet';

const EMPTY_RESULTS: SearchResult = {
  tracks: [],
  artists: [],
  albums: [],
  top_results: [],
  total: 0,
  page: 1,
  limit: 20,
};
const MOBILE_SEARCH_MIN_QUERY_LENGTH = 2;
const MOBILE_SEARCH_DEBOUNCE_MS = 400;
type SearchSort = 'relevance' | 'title' | 'artist' | 'rating' | 'duration';
type RatingFilter = 'all' | 'unrated' | 'gte3' | 'gte4';

/** Mobile Search View is part of this module's public API. */
export default function MobileSearchView({
  onPlayTrack,
  onAddToQueue,
}: {
  onPlayTrack: (track: Track, allTracks?: Track[]) => void;
  onAddToQueue: (track: Track) => void;
}) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchResult>(EMPTY_RESULTS);
  const [loading, setLoading] = useState(false);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [page, setPage] = useState(1);
  const [sort, setSort] = useState<SearchSort>('relevance');
  const [ratingFilter, setRatingFilter] = useState<RatingFilter>('all');
  const [yearFrom, setYearFrom] = useState('');
  const [yearTo, setYearTo] = useState('');
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [error, setError] = useState('');
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const requestIdRef = useRef(0);

  useEffect(() => {
    api.playlists.list().then(setPlaylists).catch(() => {});
  }, []);

  useEffect(() => {
    setPage(1);
  }, [query, ratingFilter, sort, yearFrom, yearTo]);

  useEffect(() => {
    clearTimeout(timerRef.current);
    const trimmedQuery = query.trim();
    if (trimmedQuery.length < MOBILE_SEARCH_MIN_QUERY_LENGTH) {
      requestIdRef.current += 1;
      setLoading(false);
      setResults(EMPTY_RESULTS);
      setError('');
      return;
    }

    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;

    timerRef.current = setTimeout(() => {
      setLoading(true);
      setError('');
      api.search({
        q: trimmedQuery,
        limit: 20,
        page,
        search_mode: 'mobile_omni',
        mode: 'music',
        include_artists: true,
        include_albums: true,
        include_total: true,
        sort,
        year: yearFrom ? Number(yearFrom) : undefined,
        track_rating_filter: ratingFilter,
        album_rating_filter: ratingFilter,
        artist_rating_filter: ratingFilter,
      })
        .then(nextResults => {
          if (requestIdRef.current !== requestId) return;
          setResults(current => page === 1 ? nextResults : {
            ...nextResults,
            tracks: [...current.tracks, ...nextResults.tracks],
            artists: current.artists,
            albums: current.albums,
            top_results: current.top_results,
          });
        })
        .catch(() => {
          if (requestIdRef.current !== requestId) return;
          setResults(EMPTY_RESULTS);
          setError('Search failed. Check your connection and try again.');
        })
        .finally(() => {
          if (requestIdRef.current !== requestId) return;
          setLoading(false);
        });
    }, MOBILE_SEARCH_DEBOUNCE_MS);

    return () => clearTimeout(timerRef.current);
  }, [page, query, ratingFilter, sort, yearFrom, yearTo]);

  const trimmedQuery = query.trim();
  const showMinLengthHint = !loading
    && trimmedQuery.length > 0
    && trimmedQuery.length < MOBILE_SEARCH_MIN_QUERY_LENGTH;
  const showEmptyState = !loading
    && trimmedQuery.length >= MOBILE_SEARCH_MIN_QUERY_LENGTH
    && results.tracks.length === 0
    && results.artists.length === 0
    && results.albums.length === 0
    && (results.top_results?.length ?? 0) === 0;
  const filtersActive = ratingFilter !== 'all' || Boolean(yearFrom || yearTo);
  const trackActions = useMobileTrackActions<Track>({ playlists, onPlayTrack, onAddToQueue });

  const openTopResult = (item: NonNullable<SearchResult['top_results']>[number]) => {
    const track = results.tracks.find(candidate => candidate.id === item.id);
    if (track) onPlayTrack(track, results.tracks);
  };

  return (
    <main
      aria-busy={loading}
      style={{ ...phase2.mobilePage, display: 'grid', alignContent: 'start', gap: 16 }}
    >
      <header style={hybridMobileContentStyles.pageHeader}>
        <div style={hybridMobileContentStyles.eyebrow}>Discovery</div>
        <h1 style={hybridMobileContentStyles.pageTitle}>Search</h1>
        <p style={hybridMobileContentStyles.pageBody}>
          Find songs, artists, and albums across your whole library.
        </p>
      </header>

      <input
        value={query}
        onChange={event => setQuery(event.target.value)}
        placeholder="Songs, artists, albums"
        aria-label="Search your music library"
        style={hybridMobileContentStyles.field}
      />

      {trimmedQuery.length >= MOBILE_SEARCH_MIN_QUERY_LENGTH ? (
        <div aria-label="Search sorting and filters" style={hybridMobileContentStyles.chipRow}>
          {(['relevance', 'title', 'artist', 'rating', 'duration'] as SearchSort[]).map(nextSort => (
            <button
              key={nextSort}
              type="button"
              aria-pressed={sort === nextSort}
              style={{
                ...hybridMobileContentStyles.chip,
                ...(sort === nextSort ? hybridMobileContentStyles.chipActive : {}),
              }}
              onClick={() => setSort(nextSort)}
            >
              {nextSort[0].toUpperCase() + nextSort.slice(1)}
            </button>
          ))}
          <button
            type="button"
            aria-expanded={filtersOpen}
            aria-pressed={filtersActive}
            style={{
              ...hybridMobileContentStyles.chip,
              ...(filtersActive ? hybridMobileContentStyles.chipActive : {}),
            }}
            onClick={() => setFiltersOpen(true)}
          >
            Filter
          </button>
        </div>
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
      {loading ? (
        <div role="status" style={hybridMobileContentStyles.feedback}>Searching…</div>
      ) : null}
      {!loading && !trimmedQuery ? (
        <div role="status" style={hybridMobileContentStyles.feedback}>
          Type at least {MOBILE_SEARCH_MIN_QUERY_LENGTH} characters to search your library.
        </div>
      ) : null}
      {showMinLengthHint ? (
        <div role="status" style={hybridMobileContentStyles.feedback}>
          Keep typing to search your library.
        </div>
      ) : null}

      {(results.top_results?.length ?? 0) > 0 ? (
        <section aria-labelledby="mobile-search-top-results" style={styles.section}>
          <h2 id="mobile-search-top-results" style={styles.sectionTitle}>Top Results</h2>
          <div style={hybridMobileContentStyles.list}>
            {(results.top_results ?? []).map(item => (
              <button
                key={`${item.type}-${item.id}`}
                type="button"
                style={hybridMobileContentStyles.listRow}
                onClick={() => openTopResult(item)}
                aria-label={`Open ${item.title}`}
              >
                <span style={hybridMobileContentStyles.listArtwork}>
                  <span aria-hidden="true" style={hybridMobileContentStyles.listArtworkFallback}>
                    {item.type.slice(0, 1).toUpperCase()}
                  </span>
                </span>
                <span style={hybridMobileContentStyles.listMeta}>
                  <span style={hybridMobileContentStyles.listTitle}>{item.title}</span>
                  <span style={hybridMobileContentStyles.listSubtitle}>
                    {item.subtitle || item.type}
                  </span>
                </span>
                <span style={hybridMobileContentStyles.listBadge}>{item.type}</span>
              </button>
            ))}
          </div>
        </section>
      ) : null}

      {results.artists.length > 0 ? (
        <section aria-labelledby="mobile-search-artists" style={styles.section}>
          <h2 id="mobile-search-artists" style={styles.sectionTitle}>Artists</h2>
          <div style={hybridMobileContentStyles.list}>
            {results.artists.map(artist => (
              <div key={artist.id} style={{ ...hybridMobileContentStyles.listRow, cursor: 'default' }}>
                <span style={{ ...hybridMobileContentStyles.listArtwork, borderRadius: 16 }}>
                  <ArtImage
                    src={api.artistPhotoUrl(artist.id, 300)}
                    alt=""
                    imgStyle={hybridMobileContentStyles.listArtworkImage}
                  />
                </span>
                <span style={hybridMobileContentStyles.listMeta}>
                  <span style={hybridMobileContentStyles.listTitle}>{artist.name}</span>
                  <span style={hybridMobileContentStyles.listSubtitle}>Artist</span>
                </span>
                <span style={hybridMobileContentStyles.listBadge}>Artist</span>
              </div>
            ))}
          </div>
        </section>
      ) : null}

      {results.albums.length > 0 ? (
        <section aria-labelledby="mobile-search-albums" style={styles.section}>
          <h2 id="mobile-search-albums" style={styles.sectionTitle}>Albums</h2>
          <div style={hybridMobileContentStyles.list}>
            {results.albums.map(album => (
              <div key={album.id} style={{ ...hybridMobileContentStyles.listRow, cursor: 'default' }}>
                <span style={hybridMobileContentStyles.listArtwork}>
                  <ArtImage
                    src={api.albumArtUrl(album.id, 300)}
                    alt=""
                    imgStyle={hybridMobileContentStyles.listArtworkImage}
                  />
                </span>
                <span style={hybridMobileContentStyles.listMeta}>
                  <span style={hybridMobileContentStyles.listTitle}>{album.title}</span>
                  <span style={hybridMobileContentStyles.listSubtitle}>
                    {album.album_artist || album.artist || 'Unknown artist'}
                  </span>
                </span>
                <span style={hybridMobileContentStyles.listBadge}>Album</span>
              </div>
            ))}
          </div>
        </section>
      ) : null}

      {!loading && results.tracks.length > 0 ? (
        <section aria-labelledby="mobile-search-tracks" style={styles.section}>
          <h2 id="mobile-search-tracks" style={styles.sectionTitle}>Tracks</h2>
          <div style={hybridMobileContentStyles.list}>
            {results.tracks.map(track => (
              <div key={track.id} style={styles.trackRow}>
                <span style={hybridMobileContentStyles.listArtwork}>
                  {track.album_id ? (
                    <ArtImage
                      src={api.albumArtUrl(track.album_id, 300)}
                      alt=""
                      imgStyle={hybridMobileContentStyles.listArtworkImage}
                    />
                  ) : (
                    <span aria-hidden="true" style={hybridMobileContentStyles.listArtworkFallback}>♪</span>
                  )}
                </span>
                <div style={styles.trackContent}>
                  <button
                    type="button"
                    style={styles.trackPlay}
                    onClick={() => onPlayTrack(track, results.tracks)}
                  >
                    <span style={hybridMobileContentStyles.listTitle}>
                      {track.title || track.file_name}
                    </span>
                    <span style={hybridMobileContentStyles.listSubtitle}>
                      {[track.artist, track.album].filter(Boolean).join(' • ') || 'Unknown track'}
                    </span>
                  </button>
                  <StarRating
                    value={track.rating ?? null}
                    onChange={async rating => {
                      setResults(previous => ({
                        ...previous,
                        tracks: previous.tracks.map(candidate => (
                          candidate.id === track.id ? { ...candidate, rating } : candidate
                        )),
                      }));
                      await api.setTrackRating(track.id, rating);
                    }}
                    ariaLabel={`Rate ${track.title || track.file_name}`}
                    size="compact"
                  />
                </div>
                <button
                  type="button"
                  style={styles.actionButton}
                  onClick={() => trackActions.openForTrack(track, results.tracks)}
                  aria-label={`More actions for ${track.title || track.file_name}`}
                >
                  •••
                </button>
              </div>
            ))}
          </div>
        </section>
      ) : null}

      {showEmptyState ? (
        <div role="status" style={hybridMobileContentStyles.feedback}>No music matches.</div>
      ) : null}
      {!loading && results.hasMore ? (
        <button
          type="button"
          style={{ ...hybridMobileContentStyles.secondaryAction, justifySelf: 'stretch' }}
          onClick={() => setPage(current => current + 1)}
        >
          Load more
        </button>
      ) : null}

      {trackActions.actionsSheet}
      {trackActions.pickerSheet}
      {trackActions.createSheet}

      {filtersOpen ? (
        <MobileBottomSheet title="Search filters" onClose={() => setFiltersOpen(false)}>
          <div style={styles.filterSheet}>
            <div style={{ ...hybridMobileContentStyles.chipRow, margin: 0, padding: 0 }}>
              {([
                ['all', 'All ratings'],
                ['unrated', 'Unrated'],
                ['gte3', '3+ stars'],
                ['gte4', '4+ stars'],
              ] as Array<[RatingFilter, string]>).map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  aria-pressed={ratingFilter === value}
                  style={{
                    ...hybridMobileContentStyles.chip,
                    ...(ratingFilter === value ? hybridMobileContentStyles.chipActive : {}),
                  }}
                  onClick={() => setRatingFilter(value)}
                >
                  {label}
                </button>
              ))}
            </div>
            <div style={styles.yearGrid}>
              <input
                value={yearFrom}
                onChange={event => setYearFrom(event.target.value)}
                placeholder="From year"
                aria-label="From year"
                style={{ ...hybridMobileContentStyles.field, minHeight: 44, fontSize: 15 }}
                inputMode="numeric"
              />
              <input
                value={yearTo}
                onChange={event => setYearTo(event.target.value)}
                placeholder="To year"
                aria-label="To year"
                style={{ ...hybridMobileContentStyles.field, minHeight: 44, fontSize: 15 }}
                inputMode="numeric"
              />
            </div>
          </div>
        </MobileBottomSheet>
      ) : null}
    </main>
  );
}

const styles: Record<string, CSSProperties> = {
  section: {
    display: 'grid',
    gap: 9,
  },
  sectionTitle: {
    margin: 0,
    color: 'var(--text)',
    fontSize: 17,
    fontWeight: 800,
    letterSpacing: -0.2,
  },
  trackRow: {
    minHeight: 76,
    display: 'grid',
    gridTemplateColumns: '48px minmax(0, 1fr) 44px',
    alignItems: 'center',
    gap: 9,
    boxSizing: 'border-box',
    padding: '8px 7px 8px 9px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface)',
  },
  trackContent: {
    minWidth: 0,
    display: 'grid',
    gap: 4,
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
  actionButton: {
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
    fontSize: 15,
    letterSpacing: 1,
  },
  filterSheet: {
    display: 'grid',
    gap: 14,
  },
  yearGrid: {
    display: 'grid',
    gridTemplateColumns: '1fr 1fr',
    gap: 10,
  },
};

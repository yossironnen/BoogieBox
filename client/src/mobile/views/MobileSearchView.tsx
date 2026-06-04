/**
 * Defines mobile Mobile Search View behavior for the BoogieBox React client.
 */

import React, { useEffect, useRef, useState } from 'react';
import { api } from '../../api';
import type { Playlist, SearchResult, Track } from '../../types';
import StarRating from '../../components/StarRating';
import ArtImage from '../../components/ArtImage';
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
  const timerRef = useRef<ReturnType<typeof setTimeout>>();
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
        .then((nextResults) => {
          if (requestIdRef.current !== requestId) return;
          setResults((current) => page === 1 ? nextResults : {
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
  const showMinLengthHint = !loading && trimmedQuery.length > 0 && trimmedQuery.length < MOBILE_SEARCH_MIN_QUERY_LENGTH;
  const showEmptyState = !loading
    && trimmedQuery.length >= MOBILE_SEARCH_MIN_QUERY_LENGTH
    && results.tracks.length === 0
    && results.artists.length === 0
    && results.albums.length === 0
    && (results.top_results?.length ?? 0) === 0;
  const trackActions = useMobileTrackActions<Track>({ playlists, onPlayTrack, onAddToQueue });

  const openTopResult = (item: NonNullable<SearchResult['top_results']>[number]) => {
    const track = results.tracks.find((candidate) => candidate.id === item.id);
    if (track) onPlayTrack(track, results.tracks);
  };

  return (
    <div style={styles.page}>
      <div style={styles.hero}>
        <div style={styles.kicker}>Discovery</div>
        <div style={styles.header}>Search</div>
        <div style={styles.copy}>Search across songs, artists, and albums with grouped results tuned for quick discovery.</div>
      </div>
      <div style={styles.searchShell}>
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Songs, artists, albums" style={styles.input} />
      </div>
      {trimmedQuery.length >= MOBILE_SEARCH_MIN_QUERY_LENGTH && (
        <div style={styles.pillRow}>
          {(['relevance', 'title', 'artist', 'rating', 'duration'] as SearchSort[]).map((nextSort) => (
            <button key={nextSort} type="button" style={{ ...styles.pill, ...(sort === nextSort ? styles.pillActive : undefined) }} onClick={() => setSort(nextSort)}>
              {nextSort[0].toUpperCase() + nextSort.slice(1)}
            </button>
          ))}
          <button type="button" style={styles.pill} onClick={() => setFiltersOpen(true)}>Filter</button>
        </div>
      )}
      {error ? <div style={styles.errorBanner}>{error}</div> : null}
      {loading && <div style={styles.state}>Searching...</div>}
      {!loading && !trimmedQuery && <div style={styles.state}>Type at least {MOBILE_SEARCH_MIN_QUERY_LENGTH} characters to search your library.</div>}
      {showMinLengthHint && <div style={styles.state}>Keep typing to search your library.</div>}
      {!!results.top_results?.length && (
        <div style={styles.section}>
          <div style={styles.sectionLabel}>Top Results</div>
          {results.top_results.map((item) => (
            <button key={`${item.type}-${item.id}`} type="button" style={styles.videoButton} onClick={() => openTopResult(item)} aria-label={`Open ${item.title}`}>
              <span style={styles.videoTitle}>{item.title}</span>
              <span style={styles.videoMeta}>{item.subtitle || item.type}</span>
            </button>
          ))}
        </div>
      )}
      {!!results.artists.length && (
        <div style={styles.section}>
          <div style={styles.sectionLabel}>Artists</div>
          {results.artists.map((artist) => (
            <div key={artist.id} style={styles.compactResult}>
              <ArtImage src={api.artistPhotoUrl(artist.id, 300)} alt="" imgStyle={styles.resultThumb} />
              <span style={styles.videoTitle}>{artist.name}</span>
            </div>
          ))}
        </div>
      )}
      {!!results.albums.length && (
        <div style={styles.section}>
          <div style={styles.sectionLabel}>Albums</div>
          {results.albums.map((album) => (
            <div key={album.id} style={styles.compactResult}>
              <ArtImage src={api.albumArtUrl(album.id, 300)} alt="" imgStyle={styles.resultThumb} />
              <span style={styles.videoTitle}>{album.title}</span>
            </div>
          ))}
        </div>
      )}
      {!loading && results.tracks.map((track) => (
        <div key={track.id} style={styles.row}>
          <button type="button" style={styles.main} onClick={() => onPlayTrack(track, results.tracks)}>
            <span style={styles.title}>{track.title || track.file_name}</span>
            <span style={styles.sub}>{[track.artist, track.album].filter(Boolean).join(' * ') || 'Unknown track'}</span>
            <span onClick={(event) => event.stopPropagation()}>
              <StarRating
                value={track.rating ?? null}
                onChange={async (rating) => {
                  setResults((prev) => ({ ...prev, tracks: prev.tracks.map((t) => t.id === track.id ? { ...t, rating } : t) }));
                  await api.setTrackRating(track.id, rating);
                }}
                ariaLabel={`Rate ${track.title || track.file_name}`}
                size="compact"
              />
            </span>
          </button>
          <button type="button" style={styles.queue} onClick={(event) => { event.stopPropagation(); trackActions.openForTrack(track, results.tracks); }} aria-label={`More actions for ${track.title || track.file_name}`}>
            ...
          </button>
        </div>
      ))}
      {showEmptyState && <div style={styles.state}>No music matches.</div>}
      {!loading && results.hasMore ? <button type="button" style={styles.loadMore} onClick={() => setPage((current) => current + 1)}>Load more</button> : null}
      {trackActions.actionsSheet}
      {trackActions.pickerSheet}
      {trackActions.createSheet}
      {filtersOpen ? (
        <MobileBottomSheet title="Search filters" onClose={() => setFiltersOpen(false)}>
          <div style={styles.filterSheet}>
            <div style={styles.pillRow}>
              {([
                ['all', 'All ratings'],
                ['unrated', 'Unrated'],
                ['gte3', '3+ stars'],
                ['gte4', '4+ stars'],
              ] as Array<[RatingFilter, string]>).map(([value, label]) => (
                <button key={value} type="button" style={{ ...styles.pill, ...(ratingFilter === value ? styles.pillActive : undefined) }} onClick={() => setRatingFilter(value)}>
                  {label}
                </button>
              ))}
            </div>
            <div style={styles.yearGrid}>
              <input value={yearFrom} onChange={(event) => setYearFrom(event.target.value)} placeholder="From year" style={styles.yearInput} inputMode="numeric" />
              <input value={yearTo} onChange={(event) => setYearTo(event.target.value)} placeholder="To year" style={styles.yearInput} inputMode="numeric" />
            </div>
          </div>
        </MobileBottomSheet>
      ) : null}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  page: phase2.mobilePage,
  hero: { marginBottom: 14 },
  kicker: phase2.mobileKicker,
  header: { ...phase2.mobileTitle, marginTop: 8, marginBottom: 10 },
  copy: { fontSize: 13, lineHeight: 1.5, color: 'var(--text-muted)', maxWidth: 340, marginBottom: 18 },
  searchShell: { ...phase2.mobileHeroCard, marginBottom: 16, padding: 14 },
  input: { width: '100%', minHeight: 56, padding: '0 18px', borderRadius: 18, border: '1px solid color-mix(in srgb, var(--border) 68%, transparent)', background: 'rgba(255,255,255,0.04)', color: 'var(--text)', fontFamily: 'inherit', fontSize: 16, outline: 'none' },
  pillRow: { display: 'flex', gap: 8, overflowX: 'auto', paddingBottom: 6, marginBottom: 12 },
  pill: { minHeight: 40, padding: '0 14px', borderRadius: 999, border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text)', fontFamily: 'inherit', fontSize: 12, fontWeight: 800, whiteSpace: 'nowrap' },
  pillActive: { background: 'var(--accent)', borderColor: 'var(--accent)', color: 'var(--bg)' },
  errorBanner: { padding: 12, borderRadius: 14, border: '1px solid var(--border)', background: 'color-mix(in srgb, var(--accent) 12%, var(--surface))', color: 'var(--text)', fontSize: 13, marginBottom: 12 },
  section: { display: 'grid', gap: 10, marginBottom: 14 },
  sectionLabel: { fontSize: 11, textTransform: 'uppercase', letterSpacing: 1, color: 'var(--text-muted)', fontWeight: 700 },
  videoButton: { ...phase2.mobileMediaRow, padding: '12px 14px', display: 'grid', gap: 4, textAlign: 'left', color: 'var(--text)', fontFamily: 'inherit' },
  videoTitle: { fontSize: 14, fontWeight: 800, color: 'var(--text)' },
  videoMeta: { fontSize: 12, color: 'var(--text-muted)' },
  compactResult: { ...phase2.mobileMediaRow, minHeight: 58, padding: '8px 12px', display: 'grid', gridTemplateColumns: '42px minmax(0, 1fr)', alignItems: 'center', gap: 10 },
  resultThumb: { width: 42, height: 42, borderRadius: 10, objectFit: 'cover', display: 'block' },
  state: { color: 'var(--text-muted)', fontSize: 14, paddingTop: 14 },
  row: { display: 'flex', gap: 10, marginBottom: 12 },
  main: { ...phase2.mobileMediaRow, flex: 1, minHeight: 72, textAlign: 'left', color: 'var(--text)', padding: '14px 16px', display: 'grid', gap: 5 },
  title: { fontSize: 15, fontWeight: 800, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  sub: { fontSize: 13, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  queue: { ...phase2.mobileMediaRow, minWidth: 56, color: 'var(--text)', fontSize: 18, fontFamily: 'inherit', letterSpacing: 1 },
  loadMore: { minHeight: 48, borderRadius: 16, border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text)', fontFamily: 'inherit', fontWeight: 800, marginTop: 8 },
  filterSheet: { display: 'grid', gap: 14 },
  yearGrid: { display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 },
  yearInput: { minHeight: 44, borderRadius: 12, border: '1px solid var(--border)', background: 'var(--bg)', color: 'var(--text)', fontFamily: 'inherit', padding: '0 12px' },
};

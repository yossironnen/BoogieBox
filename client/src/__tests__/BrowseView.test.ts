/**
 * Tests Browse View.Test behavior for BoogieBox regressions.
 */

import { describe, it, expect } from 'vitest';
import {
  applyAlbumRating,
  applyArtistRating,
  applyTrackRating,
  buildLetterFirstIndexMap,
  filterAlbumsByRating,
  filterArtistsByRating,
  filterTracksByRating,
  fmtDur,
  fmtTrackDur,
  getAlbumDisplayArtist,
  getAlbumSortLabel,
  getRatingFilterLabel,
  matchesAlbumRatingTarget,
  matchesRatingFilter,
  parseArtistPhotoPayload,
  parseLastFmTopTracks,
  parseSortDir,
  shouldApplyBrowseRootFetchResult,
  sortAlbums,
  sortArtists,
  sortTracks,
  toAlphaBucket,
  toSingleGenreSelection,
  toggleGenreSelection,
} from '../components/BrowseView';
import { findTopTrackMatch, matchesTrackArtist, resolveTopTrackFromLibrarySearch } from '../artistTrackMatching';
import { groupArtistDiscographyByReleaseType } from '../releaseTypes';
import type { Album, Artist, ClientEntityId, Track } from '../types';

function makeAlbum(overrides: Partial<Album> & { title: string }): Album {
  return {
    id: '0',
    artist: null,
    album_artist: null,
    year: null,
    genre: null,
    track_count: 0,
    total_duration: null,
    ...overrides,
  };
}

const ALBUMS = [
  makeAlbum({ id: '1', title: 'Zebra', year: 2010 }),
  makeAlbum({ id: '2', title: 'Alpha', year: 2022 }),
  makeAlbum({ id: '3', title: 'Mango', year: 1995 }),
  makeAlbum({ id: '4', title: 'beta',  year: null }),
];

describe('sortAlbums', () => {
  describe('sort by title asc', () => {
    it('returns albums A→Z (case-insensitive)', () => {
      const result = sortAlbums(ALBUMS, 'title', 'asc');
      expect(result.map(a => a.title)).toEqual(['Alpha', 'beta', 'Mango', 'Zebra']);
    });
  });

  describe('sort by title desc', () => {
    it('returns albums Z→A (case-insensitive)', () => {
      const result = sortAlbums(ALBUMS, 'title', 'desc');
      expect(result.map(a => a.title)).toEqual(['Zebra', 'Mango', 'beta', 'Alpha']);
    });
  });

  describe('sort by year asc', () => {
    it('returns albums oldest first, nulls at end', () => {
      const result = sortAlbums(ALBUMS, 'year', 'asc');
      expect(result.map(a => a.year)).toEqual([1995, 2010, 2022, null]);
    });
  });

  describe('sort by year desc', () => {
    it('returns albums newest first, nulls at end', () => {
      const result = sortAlbums(ALBUMS, 'year', 'desc');
      expect(result.map(a => a.year)).toEqual([2022, 2010, 1995, null]);
    });
  });

  it('does not mutate the original array', () => {
    const original = [...ALBUMS];
    sortAlbums(ALBUMS, 'title', 'asc');
    expect(ALBUMS).toEqual(original);
  });

  it('handles empty array', () => {
    expect(sortAlbums([], 'title', 'asc')).toEqual([]);
  });

  it('handles single album', () => {
    const one = [makeAlbum({ id: '9', title: 'Solo', year: 2000 })];
    expect(sortAlbums(one, 'year', 'desc')).toHaveLength(1);
  });

  it('handles albums where all years are null — order is stable', () => {
    const noYears = [
      makeAlbum({ id: '1', title: 'C', year: null }),
      makeAlbum({ id: '2', title: 'A', year: null }),
    ];
    const result = sortAlbums(noYears, 'year', 'asc');
    // All nulls map to Infinity so comparison yields 0 — original order preserved
    expect(result.map(a => a.title)).toEqual(['C', 'A']);
  });

  it('handles albums with empty title strings', () => {
    const withEmpty = [
      makeAlbum({ id: '1', title: 'B', year: null }),
      makeAlbum({ id: '2', title: '',  year: null }),
    ];
    const result = sortAlbums(withEmpty, 'title', 'asc');
    // Empty string sorts before 'B'
    expect(result[0].title).toBe('');
  });
});

describe('groupArtistDiscographyByReleaseType', () => {
  it('groups albums into album/single/compilation with unknown values defaulting to album', () => {
    const grouped = groupArtistDiscographyByReleaseType([
      makeAlbum({ id: '1', title: 'LP One', releaseType: 'album' }),
      makeAlbum({ id: '2', title: 'EP One', releaseType: 'single' }),
      makeAlbum({ id: '3', title: 'Best Of', releaseType: 'compilation' }),
      makeAlbum({ id: '4', title: 'Unknown Type', releaseType: undefined }),
    ]);

    expect(grouped.album.map((album) => album.title)).toEqual(['LP One', 'Unknown Type']);
    expect(grouped.single.map((album) => album.title)).toEqual(['EP One']);
    expect(grouped.compilation.map((album) => album.title)).toEqual(['Best Of']);
  });
});

// ─── sortArtists ──────────────────────────────────────────────────────────────

function makeArtist(id: ClientEntityId, name: string): Artist {
  return { id, name, track_count: 0, album_count: 0 };
}

const ARTISTS = [
  makeArtist('1', 'Zeppelin'),
  makeArtist('2', 'abba'),
  makeArtist('3', 'Madonna'),
  makeArtist('4', 'the Beatles'),
];

describe('sortArtists', () => {
  it('sorts A→Z case-insensitively when dir is asc', () => {
    const result = sortArtists(ARTISTS, 'asc');
    expect(result.map(a => a.name)).toEqual(['abba', 'Madonna', 'the Beatles', 'Zeppelin']);
  });

  it('sorts Z→A case-insensitively when dir is desc', () => {
    const result = sortArtists(ARTISTS, 'desc');
    expect(result.map(a => a.name)).toEqual(['Zeppelin', 'the Beatles', 'Madonna', 'abba']);
  });

  it('does not mutate the original array', () => {
    const original = [...ARTISTS];
    sortArtists(ARTISTS, 'asc');
    expect(ARTISTS).toEqual(original);
  });

  it('handles empty array', () => {
    expect(sortArtists([], 'asc')).toEqual([]);
  });

  it('handles single artist', () => {
    expect(sortArtists([makeArtist('1', 'Solo')], 'desc')).toHaveLength(1);
  });
});

describe('parseLastFmTopTracks', () => {
  it('parses and returns top 5 tracks sorted by playcount desc', () => {
    const data = {
      toptracks: {
        track: [
          { name: 'Song C', playcount: '100', listeners: '10', url: 'https://x/c' },
          { name: 'Song A', playcount: '500', listeners: '20', url: 'https://x/a' },
          { name: 'Song B', playcount: '300', listeners: '15', url: 'https://x/b' },
          { name: 'Song D', playcount: '200', listeners: '12' },
          { name: 'Song E', playcount: '50' },
          { name: 'Song F', playcount: '40' },
        ],
      },
    };

    const result = parseLastFmTopTracks(data, 5);
    expect(result.map(t => t.name)).toEqual(['Song A', 'Song B', 'Song D', 'Song C', 'Song E']);
    expect(result).toHaveLength(5);
    expect(result[0].playcount).toBe(500);
  });

  it('filters out invalid entries', () => {
    const data = {
      toptracks: {
        track: [
          { name: '', playcount: '100' },
          { name: 'Bad Number', playcount: 'abc' },
          { name: 'Zero', playcount: '0' },
          { name: 'Valid', playcount: '12' },
        ],
      },
    };

    expect(parseLastFmTopTracks(data)).toEqual([
      { name: 'Valid', playcount: 12, listeners: undefined, url: undefined },
    ]);
  });

  it('returns empty array when payload is missing track list', () => {
    expect(parseLastFmTopTracks({})).toEqual([]);
    expect(parseLastFmTopTracks({ toptracks: {} })).toEqual([]);
    expect(parseLastFmTopTracks({ toptracks: { track: null } })).toEqual([]);
  });
});

describe('parseArtistPhotoPayload', () => {
  it('returns payload when url and source are valid', () => {
    expect(parseArtistPhotoPayload({ url: 'https://img/deezer.jpg', source: 'deezer' })).toEqual({
      url: 'https://img/deezer.jpg',
      source: 'deezer',
    });
    expect(parseArtistPhotoPayload({ url: 'https://img/spotify.jpg', source: 'spotify' })).toEqual({
      url: 'https://img/spotify.jpg',
      source: 'spotify',
    });
  });

  it('returns null for missing/invalid fields', () => {
    expect(parseArtistPhotoPayload({})).toBeNull();
    expect(parseArtistPhotoPayload({ url: '' })).toBeNull();
    expect(parseArtistPhotoPayload({ url: 123, source: 'deezer' })).toBeNull();
    expect(parseArtistPhotoPayload({ url: 'https://x', source: 'other' })).toBeNull();
  });
});

function makeTrack(overrides: Partial<Track> & { id: ClientEntityId; file_name: string }): Track {
  return {
    file_path: `C:\\music\\${overrides.file_name}`,
    file_size: null,
    format: null,
    duration: null,
    bitrate: null,
    sample_rate: null,
    channels: null,
    title: null,
    artist: null,
    album: null,
    library_name: null,
    track_number: null,
    disc_number: null,
    year: null,
    genre: null,
    composer: null,
    comment: null,
    bpm: null,
    scanned_at: new Date().toISOString(),
    ...overrides,
  };
}

describe('findTopTrackMatch', () => {
  it('returns the first exact title match', () => {
    const candidates: Track[] = [
      makeTrack({ id: '1', file_name: 'other.flac', title: 'My Song', artist: 'Someone Else' }),
      makeTrack({ id: '2', file_name: 'mine.flac', title: 'My Song', artist: 'The Artist' }),
    ];

    const result = findTopTrackMatch(candidates, 'My Song');
    expect(result?.id).toBe('1');
  });

  it('falls back to title contains when exact is missing', () => {
    const candidates: Track[] = [
      makeTrack({ id: '3', file_name: 'extended.flac', title: 'My Song (Extended Mix)', artist: 'The Artist' }),
    ];

    const result = findTopTrackMatch(candidates, 'My Song');
    expect(result?.id).toBe('3');
  });

  it('matches the file name when title metadata is absent', () => {
    const candidates: Track[] = [
      makeTrack({ id: '5', file_name: 'My Song.flac', title: null, artist: 'The Artist' }),
    ];

    expect(findTopTrackMatch(candidates, 'My Song flac')?.id).toBe('5');
  });

  it('returns null when no reasonable match exists', () => {
    const candidates: Track[] = [
      makeTrack({ id: '4', file_name: 'different.flac', title: 'Completely Different', artist: 'Another Artist' }),
    ];

    const result = findTopTrackMatch(candidates, 'My Song');
    expect(result).toBeNull();
  });
});

describe('matchesTrackArtist', () => {
  it('matches by track artist tag', () => {
    expect(matchesTrackArtist('ABBA', 'ABBA')).toBe(true);
  });

  it('is case-insensitive and punctuation-insensitive', () => {
    expect(matchesTrackArtist('AC/DC', 'ac dc')).toBe(true);
  });

  it('returns false for non-matching metadata', () => {
    expect(matchesTrackArtist('ABBA', 'Erasure')).toBe(false);
  });

  it('returns false when track artist is empty', () => {
    expect(matchesTrackArtist('ABBA', null)).toBe(false);
  });
});

describe('resolveTopTrackFromLibrarySearch', () => {
  it('picks a same-title track only when artist tag matches', () => {
    const tracks: Track[] = [
      makeTrack({ id: '1', file_name: 'wrong.flac', title: 'The Winner Takes It All', artist: 'Erasure' }),
      makeTrack({ id: '2', file_name: 'right.flac', title: 'The Winner Takes It All', artist: 'abba' }),
    ];
    const result = resolveTopTrackFromLibrarySearch('Abba', 'The Winner Takes It All', tracks);
    expect(result?.id).toBe('2');
  });

  it('returns null when title exists but no artist tag matches', () => {
    const tracks: Track[] = [
      makeTrack({ id: '3', file_name: 'only.flac', title: 'The Winner Takes It All', artist: 'Erasure' }),
    ];
    const result = resolveTopTrackFromLibrarySearch('Abba', 'The Winner Takes It All', tracks);
    expect(result).toBeNull();
  });
});

describe('toggleGenreSelection', () => {
  it('adds a genre when it is not currently selected', () => {
    expect(toggleGenreSelection(['Rock'], 'Jazz')).toEqual(['Rock', 'Jazz']);
  });

  it('removes a genre when already selected (case-insensitive)', () => {
    expect(toggleGenreSelection(['Rock', 'Jazz'], 'jazz')).toEqual(['Rock']);
  });

  it('ignores empty genres', () => {
    expect(toggleGenreSelection(['Rock'], '   ')).toEqual(['Rock']);
  });
});

describe('toSingleGenreSelection', () => {
  it('returns a single-item array when genre is valid', () => {
    expect(toSingleGenreSelection('Rock')).toEqual(['Rock']);
  });

  it('trims incoming genre values', () => {
    expect(toSingleGenreSelection('  Jazz  ')).toEqual(['Jazz']);
  });

  it('returns empty array for blank genres', () => {
    expect(toSingleGenreSelection('   ')).toEqual([]);
  });
});

describe('shouldApplyBrowseRootFetchResult', () => {
  it('returns true for latest fetch tokens', () => {
    expect(shouldApplyBrowseRootFetchResult(3, 3)).toBe(true);
  });

  it('returns false for stale fetch tokens', () => {
    expect(shouldApplyBrowseRootFetchResult(2, 3)).toBe(false);
  });
});

describe('BrowseView data formatting and local update helpers', () => {
  it('normalizes alpha buckets and keeps the first index for each bucket', () => {
    expect([toAlphaBucket(null), toAlphaBucket('  '), toAlphaBucket('7even'), toAlphaBucket('abba'), toAlphaBucket('Zulu')])
      .toEqual(['#', '#', '#', 'A', 'Z']);
    expect(buildLetterFirstIndexMap(
      [{ name: 'Beta' }, { name: 'alpha' }, { name: 'Another' }, { name: '' }],
      item => item.name,
    )).toEqual({ B: 0, A: 1, '#': 3 });
  });

  it('covers every rating filter and its labels', () => {
    expect(matchesRatingFilter(null, 'all')).toBe(true);
    expect(matchesRatingFilter(1, 'rated')).toBe(true);
    expect(matchesRatingFilter(null, 'rated')).toBe(false);
    expect(matchesRatingFilter(null, 'unrated')).toBe(true);
    expect(matchesRatingFilter(4, 'gte4')).toBe(true);
    expect(matchesRatingFilter(3.5, 'gte4')).toBe(false);
    expect(matchesRatingFilter(3, 'gte3')).toBe(true);
    expect(matchesRatingFilter(undefined, 'gte3')).toBe(false);
    expect(matchesRatingFilter(5, 'unknown' as never)).toBe(true);
    expect(['all', 'rated', 'unrated', 'gte4', 'gte3'].map(value => getRatingFilterLabel(value as never)))
      .toEqual(['All', 'Rated', 'Unrated', '4+', '3+']);
    expect(getRatingFilterLabel('unknown' as never)).toBe('All');
  });

  it('filters each entity kind and formats every sort label', () => {
    const albums = [
      makeAlbum({ id: '1', title: 'Rated', rating: 4 }),
      makeAlbum({ id: '2', title: 'Unrated', rating: null }),
    ];
    const artists = [
      { id: '1', name: 'Rated', track_count: 1, album_count: 1, rating: 3 },
      { id: '2', name: 'Unrated', track_count: 1, album_count: 1, rating: null },
    ];
    const tracks = [
      makeTrack({ id: '1', file_name: 'rated.flac', rating: 5 }),
      makeTrack({ id: '2', file_name: 'unrated.flac', rating: null }),
    ];
    expect(filterAlbumsByRating(albums, 'rated').map(item => item.id)).toEqual(['1']);
    expect(filterArtistsByRating(artists, 'unrated').map(item => item.id)).toEqual(['2']);
    expect(filterTracksByRating(tracks, 'gte4').map(item => item.id)).toEqual(['1']);
    expect(getAlbumSortLabel('title', 'asc')).toBe('Name ↑');
    expect(getAlbumSortLabel('year', 'desc')).toBe('Year ↓');
    expect(getAlbumSortLabel('rating', 'asc')).toBe('Rating ↑');
    expect(parseSortDir('desc')).toBe('desc');
    expect(parseSortDir('asc')).toBe('asc');
    expect(parseSortDir(null)).toBe('asc');
  });

  it('sorts album-order and rating-order tracks through tie and null cases', () => {
    const tracks = [
      makeTrack({ id: '1', file_name: 'z.flac', title: '', disc_number: 2, track_number: 1, rating: null }),
      makeTrack({ id: '2', file_name: 'b.flac', title: 'Beta', disc_number: 1, track_number: null, rating: 4 }),
      makeTrack({ id: '3', file_name: 'a.flac', title: 'Alpha', disc_number: 1, track_number: 2, rating: 4 }),
      makeTrack({ id: '4', file_name: 'c.flac', title: 'Charlie', disc_number: 1, track_number: 2, rating: null }),
    ];
    expect(sortTracks(tracks, 'album', 'asc').map(item => item.id)).toEqual(['3', '4', '2', '1']);
    expect(sortTracks(tracks, 'rating', 'desc').map(item => item.id)).toEqual(['3', '2', '1', '4']);
    expect(sortTracks(tracks, 'rating', 'asc').map(item => item.id)).toEqual(['3', '2', '1', '4']);

    const ratingBranches = [
      makeTrack({ id: '5', file_name: 'fallback-z.flac', title: '', track_number: null, rating: null }),
      makeTrack({ id: '6', file_name: 'fallback-a.flac', title: null, track_number: null, rating: null }),
      makeTrack({ id: '7', file_name: 'low.flac', track_number: 9, rating: 1 }),
      makeTrack({ id: '8', file_name: 'high.flac', track_number: 3, rating: 5 }),
      makeTrack({ id: '9', file_name: 'high-later.flac', track_number: 8, rating: 5 }),
    ];
    expect(sortTracks(ratingBranches, 'rating', 'asc').map(item => item.id))
      .toEqual(['7', '8', '9', '6', '5']);
    expect(sortTracks(ratingBranches, 'rating', 'desc').map(item => item.id))
      .toEqual(['8', '9', '7', '6', '5']);
  });

  it('formats durations and resolves grouped album artists', () => {
    const album = makeAlbum({ id: '1', title: 'Album', artist: 'Track Artist', album_artist: 'Album Artist' });
    expect([fmtDur(null), fmtDur(0), fmtDur(45), fmtDur(65), fmtDur(3665)])
      .toEqual(['', '', '45s', '1m 5s', '1h 1m']);
    expect(fmtTrackDur(null)).toBe('–');
    expect(fmtTrackDur(65)).toBe('1:05');
    expect(getAlbumDisplayArtist(album, 'album_artist')).toBe('Album Artist');
    expect(getAlbumDisplayArtist({ ...album, album_artist: null }, 'album_artist')).toBe('Track Artist');
    expect(getAlbumDisplayArtist(album, 'artist')).toBe('Track Artist');
    expect(getAlbumDisplayArtist({ ...album, artist: null }, 'artist')).toBeNull();
  });

  it('matches fallback album identities and applies immutable rating updates', () => {
    const album = makeAlbum({ id: '1', title: ' Album ', album_artist: ' Artist ', rating: null });
    const sameName = makeAlbum({ id: '2', title: 'album', album_artist: 'artist', rating: 2 });
    const other = makeAlbum({ id: '3', title: 'Other', album_artist: null, rating: 1 });
    expect(matchesAlbumRatingTarget(album, album)).toBe(true);
    expect(matchesAlbumRatingTarget(sameName, album)).toBe(true);
    expect(matchesAlbumRatingTarget(other, album)).toBe(false);
    expect(applyAlbumRating([sameName, other], album, 5).map(item => item.rating)).toEqual([5, 1]);

    const tracks = [makeTrack({ id: '1', file_name: 'one.flac', rating: null }), makeTrack({ id: '2', file_name: 'two.flac', rating: 2 })];
    expect(applyTrackRating(tracks, '1', 4).map(item => item.rating)).toEqual([4, 2]);
    const artists = [makeArtist('1', 'One'), makeArtist('2', 'Two')];
    expect(applyArtistRating(artists, '2', 3).map(item => item.rating ?? null)).toEqual([null, 3]);
  });

  it('sorts albums by rating with nulls, ties, and both directions', () => {
    const albums = [
      makeAlbum({ id: '1', title: 'Zulu', rating: null }),
      makeAlbum({ id: '2', title: 'Alpha', rating: null }),
      makeAlbum({ id: '3', title: 'Beta', rating: 4 }),
      makeAlbum({ id: '4', title: 'Able', rating: 4 }),
      makeAlbum({ id: '5', title: 'Low', rating: 2 }),
    ];
    expect(sortAlbums(albums, 'rating', 'asc').map(item => item.id)).toEqual(['5', '4', '3', '2', '1']);
    expect(sortAlbums(albums, 'rating', 'desc').map(item => item.id)).toEqual(['4', '3', '5', '2', '1']);
  });
});


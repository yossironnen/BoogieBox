/**
 * Tests App Refresh.Test behavior for BoogieBox regressions.
 */

import { describe, expect, it } from 'vitest';
import {
  createBrowseGenreRequest,
  extractThemeSettings,
  findAlbumByName,
  findArtistByName,
  fulfilledValue,
  getThemeTextureVars,
  normalizeAlbumLookupTitle,
  normalizeArtistLookupName,
  parseThemeSettings,
  shouldRecordTrackPlay,
} from '../App';
import { DEFAULT_SETTINGS } from '../types';

describe('fulfilledValue', () => {
  it('returns fulfilled values', () => {
    const result = fulfilledValue({ status: 'fulfilled', value: 42 });
    expect(result).toBe(42);
  });

  it('returns null for rejected results', () => {
    const result = fulfilledValue<number>({ status: 'rejected', reason: new Error('x') });
    expect(result).toBeNull();
  });
});

describe('parseThemeSettings', () => {
  it('returns null for invalid JSON', () => {
    expect(parseThemeSettings('{bad-json')).toBeNull();
  });

  it('returns only valid theme values', () => {
    const parsed = parseThemeSettings(JSON.stringify({
      colorBg: '#101010',
      colorSurface: 'invalid',
      colorBorder: '#202020',
      colorAccent: '#303030',
      colorText: '#404040',
      colorTextMuted: '#505050',
      bgTexture: 'wood',
      fontFamily: 'Inter',
      extra: 'ignored',
    }));
    expect(parsed).toEqual({
      colorBg: '#101010',
      colorBorder: '#202020',
      colorAccent: '#303030',
      colorText: '#404040',
      colorTextMuted: '#505050',
      bgTexture: 'wood',
      fontFamily: 'Inter',
    });
  });

  it('returns null when no valid values exist', () => {
    expect(parseThemeSettings(JSON.stringify({ colorBg: 'blue', bgTexture: 'grainy', fontFamily: '   ' }))).toBeNull();
  });
});

describe('extractThemeSettings', () => {
  it('extracts appearance fields only', () => {
    const extracted = extractThemeSettings(DEFAULT_SETTINGS);
    expect(extracted).toEqual({
      colorBg: DEFAULT_SETTINGS.colorBg,
      colorSurface: DEFAULT_SETTINGS.colorSurface,
      colorBorder: DEFAULT_SETTINGS.colorBorder,
      colorAccent: DEFAULT_SETTINGS.colorAccent,
      colorText: DEFAULT_SETTINGS.colorText,
      colorTextMuted: DEFAULT_SETTINGS.colorTextMuted,
      bgTexture: DEFAULT_SETTINGS.bgTexture,
      fontFamily: DEFAULT_SETTINGS.fontFamily,
    });
    expect(Object.prototype.hasOwnProperty.call(extracted, 'lastfmKey')).toBe(false);
  });
});

describe('getThemeTextureVars', () => {
  it('returns wood texture layers for wood mode', () => {
    const result = getThemeTextureVars('wood');
    expect(result.image).toContain('linear-gradient');
    expect(result.size).toContain('300px');
  });

  it('returns none for non-wood modes', () => {
    expect(getThemeTextureVars('none')).toEqual({ image: 'none', size: 'auto' });
    expect(getThemeTextureVars('unknown')).toEqual({ image: 'none', size: 'auto' });
  });
});

describe('createBrowseGenreRequest', () => {
  it('returns a request object for non-empty genres', () => {
    expect(createBrowseGenreRequest('Rock', 123)).toEqual({ genre: 'Rock', token: 123 });
  });

  it('trims genres and rejects empty input', () => {
    expect(createBrowseGenreRequest('  Jazz  ', 55)).toEqual({ genre: 'Jazz', token: 55 });
    expect(createBrowseGenreRequest('   ', 55)).toBeNull();
  });
});

describe('shouldRecordTrackPlay', () => {
  it('returns true for non-empty string track ids', () => {
    expect(shouldRecordTrackPlay({ id: '7' })).toBe(true);
  });

  it('returns false for missing/invalid ids', () => {
    expect(shouldRecordTrackPlay(undefined)).toBe(false);
    expect(shouldRecordTrackPlay(null)).toBe(false);
    expect(shouldRecordTrackPlay({ id: '0' })).toBe(false);
    expect(shouldRecordTrackPlay({ id: '' })).toBe(false);
  });
});

describe('artist lookup helpers', () => {
  it('normalizes artist names for stable comparisons', () => {
    expect(normalizeArtistLookupName('  The   Avalanches  ')).toBe('the avalanches');
  });

  it('finds artist by normalized name match', () => {
    const artists = [
      { id: '1', name: 'The Avalanches', track_count: 20, album_count: 2 },
      { id: '2', name: 'Boards of Canada', track_count: 30, album_count: 5 },
    ];

    const match = findArtistByName(artists as any, '  the   avalanches  ');
    expect(match?.id).toBe('1');
  });

  it('returns null when no normalized match is found', () => {
    const artists = [{ id: '1', name: 'Tycho', track_count: 10, album_count: 1 }];
    expect(findArtistByName(artists as any, 'Bonobo')).toBeNull();
  });
});

describe('album lookup helpers', () => {
  it('normalizes album titles for stable comparisons', () => {
    expect(normalizeAlbumLookupTitle('  Dive   Deluxe  ')).toBe('dive deluxe');
  });

  it('finds album by normalized title and preferred artist match', () => {
    const albums = [
      { id: '1', title: 'Dive', artist: 'Random Artist', album_artist: 'Various Artists', year: 2011, genre: 'Electronic', track_count: 12 },
      { id: '2', title: 'Dive', artist: 'Tycho', album_artist: 'Tycho', year: 2011, genre: 'Electronic', track_count: 12 },
      { id: '3', title: 'Epoch', artist: 'Tycho', album_artist: 'Tycho', year: 2016, genre: 'Electronic', track_count: 11 },
    ];

    const match = findAlbumByName(albums as any, '  dive  ', 'tycho');
    expect(match?.id).toBe('2');
  });

  it('falls back to first title match when artist does not match', () => {
    const albums = [
      { id: '10', title: 'Dive', artist: 'A', album_artist: 'A', year: 2011, genre: 'Electronic', track_count: 12 },
      { id: '11', title: 'Dive', artist: 'B', album_artist: 'B', year: 2011, genre: 'Electronic', track_count: 12 },
    ];

    const match = findAlbumByName(albums as any, 'Dive', 'No Match');
    expect(match?.id).toBe('10');
  });

  it('returns null when no album title match is found', () => {
    const albums = [{ id: '1', title: 'Epoch', artist: 'Tycho', album_artist: 'Tycho', year: 2016, genre: 'Electronic', track_count: 11 }];
    expect(findAlbumByName(albums as any, 'Dive', 'Tycho')).toBeNull();
  });
});


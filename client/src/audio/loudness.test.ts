/**
 * Tests the loudness levelling maths: modes, auto album context, peak clamp and pre-amp.
 */

import { describe, expect, it } from 'vitest';
import { dbToLinear, parseLoudnessMode, parseLoudnessPreamp, pickGainDb, sameAlbum } from './loudness';

const loud = { trackGainDb: -8, trackPeak: 0.99, albumGainDb: -6, albumPeak: 1 };
const quiet = { trackGainDb: 4, trackPeak: 0.5, albumGainDb: 3, albumPeak: 0.6 };

describe('pickGainDb', () => {
  it('plays at unity when off or without data', () => {
    expect(pickGainDb('off', 3, loud, true)).toBe(0);
    expect(pickGainDb('track', 3, null, false)).toBe(0);
    expect(pickGainDb('track', 0, { trackGainDb: Number.NaN }, false)).toBe(0);
  });

  it('uses track or album gain by mode, and album gain in auto only with album context', () => {
    expect(pickGainDb('track', 0, loud, true)).toBe(-8);
    expect(pickGainDb('album', 0, loud, false)).toBe(-6);
    expect(pickGainDb('auto', 0, loud, true)).toBe(-6);
    expect(pickGainDb('auto', 0, loud, false)).toBe(-8);
    expect(pickGainDb('album', 0, { trackGainDb: -5, trackPeak: 0.5 }, true)).toBe(-5);
  });

  it('adds the pre-amp and clamps to the peak', () => {
    expect(pickGainDb('track', 2, loud, false)).toBe(-6);
    // Peak 0.5 allows +5.02 dB at most (−1 dBTP headroom).
    expect(pickGainDb('track', 6, quiet, false)).toBe(5.02);
    expect(pickGainDb('track', 0, { trackGainDb: 3, trackPeak: 0.8 }, false)).toBe(0.94);
    expect(pickGainDb('album', 0, { trackGainDb: 0, albumGainDb: 9, albumPeak: null, trackPeak: 0.25 }, true)).toBe(9);
    expect(pickGainDb('track', 0, { trackGainDb: 9 }, false)).toBe(6);
    expect(pickGainDb('track', 0, { trackGainDb: -9 }, false)).toBe(-9);
  });
});

describe('helpers', () => {
  it('parses settings defensively', () => {
    expect(parseLoudnessMode('auto')).toBe('auto');
    expect(parseLoudnessMode('loud')).toBe('off');
    expect(parseLoudnessMode(undefined)).toBe('off');
    expect(parseLoudnessPreamp('1.5')).toBe(1.5);
    expect(parseLoudnessPreamp('9')).toBe(6);
    expect(parseLoudnessPreamp('-1.3')).toBe(-1.5);
    expect(parseLoudnessPreamp('x')).toBe(0);
    expect(parseLoudnessPreamp('')).toBe(0);
  });

  it('converts dB and compares albums', () => {
    expect(dbToLinear(0)).toBe(1);
    expect(dbToLinear(-20)).toBeCloseTo(0.1);
    expect(sameAlbum({ album_id: 1 }, { album_id: '1' })).toBe(true);
    expect(sameAlbum({ album_id: 1 }, { album_id: 2 })).toBe(false);
    expect(sameAlbum({ album: 'A', album_artist: 'X' }, { album: 'A', album_artist: 'X' })).toBe(true);
    expect(sameAlbum({ album: 'A', album_artist: 'X' }, { album: 'A', album_artist: 'Y' })).toBe(false);
    expect(sameAlbum({ album: '' }, { album: '' })).toBe(false);
    expect(sameAlbum(null, { album_id: 1 })).toBe(false);
  });
});

/**
 * Tests Types.Test behavior for BoogieBox regressions.
 */

import { describe, it, expect } from 'vitest';
import { DEFAULT_SETTINGS, FONT_OPTIONS } from '../types';
import type { SearchResult } from '../types';

describe('DEFAULT_SETTINGS', () => {
  it('contains all required keys', () => {
    const keys: (keyof typeof DEFAULT_SETTINGS)[] = [
      'colorBg', 'colorSurface', 'colorBorder', 'colorAccent',
      'colorText', 'colorTextMuted', 'bgTexture', 'fontFamily', 'lastfmKey',
      'dlnaEnabled', 'dlnaFriendlyName', 'dlnaPort',
      'transcodeQuality',
      'waveformGenerateOnMissing', 'waveformBackgroundEnabled', 'waveformBackgroundFrequencyHours', 'waveformBackgroundBatchSize',
      'scanDebugLoggingEnabled',
    ];
    keys.forEach(k => expect(DEFAULT_SETTINGS).toHaveProperty(k));
  });

  it('has the expected default accent color', () => {
    expect(DEFAULT_SETTINGS.colorAccent).toBe('#d08b52');
  });

  it('has the expected default background color', () => {
    expect(DEFAULT_SETTINGS.colorBg).toBe('#161312');
  });

  it('has the softer sans stack as the default font', () => {
    expect(DEFAULT_SETTINGS.fontFamily).toBe('Aptos, "Segoe UI Variable", "Segoe UI", Inter, system-ui, sans-serif');
  });

  it('has an empty lastfmKey by default', () => {
    expect(DEFAULT_SETTINGS.lastfmKey).toBe('');
  });

  it('has no texture by default', () => {
    expect(DEFAULT_SETTINGS.bgTexture).toBe('none');
  });

  it('has DLNA disabled with expected default identity and port', () => {
    expect(DEFAULT_SETTINGS.dlnaEnabled).toBe('false');
    expect(DEFAULT_SETTINGS.dlnaFriendlyName).toBe('BoogieBox');
    expect(DEFAULT_SETTINGS.dlnaPort).toBe('8200');
    expect(DEFAULT_SETTINGS.transcodeQuality).toBe('low');
  });

  it('has waveform generation enabled on missing tracks and background mapping disabled by default', () => {
    expect(DEFAULT_SETTINGS.waveformGenerateOnMissing).toBe('true');
    expect(DEFAULT_SETTINGS.waveformBackgroundEnabled).toBe('false');
    expect(DEFAULT_SETTINGS.waveformBackgroundFrequencyHours).toBe('24');
    expect(DEFAULT_SETTINGS.waveformBackgroundBatchSize).toBe('100');
  });

  it('keeps scan debug logging disabled by default', () => {
    expect(DEFAULT_SETTINGS.scanDebugLoggingEnabled).toBe('false');
  });

  it('all color values are valid hex strings', () => {
    const hexRe = /^#[0-9a-f]{6}$/i;
    ['colorBg', 'colorSurface', 'colorBorder', 'colorAccent', 'colorText', 'colorTextMuted'].forEach(k => {
      expect(DEFAULT_SETTINGS[k as keyof typeof DEFAULT_SETTINGS]).toMatch(hexRe);
    });
  });
});

describe('SearchResult', () => {
  it('accepts artists and albums arrays', () => {
    const r: SearchResult = {
      tracks: [], total: 0, page: 1, limit: 50,
      artists: [{ id: '1', name: 'Test Artist', track_count: 5, album_count: 2 }],
      albums:  [{ id: '1', title: 'Test Album', artist: null, album_artist: null, year: null, genre: null, track_count: 5 }],
    };
    expect(r.artists).toHaveLength(1);
    expect(r.albums).toHaveLength(1);
  });
});

describe('FONT_OPTIONS', () => {
  it('contains at least one option', () => {
    expect(FONT_OPTIONS.length).toBeGreaterThan(0);
  });

  it('every option has a non-empty label and value', () => {
    FONT_OPTIONS.forEach(opt => {
      expect(typeof opt.label).toBe('string');
      expect(typeof opt.value).toBe('string');
      expect(opt.label.length).toBeGreaterThan(0);
      expect(opt.value.length).toBeGreaterThan(0);
    });
  });

  it('includes the default font stack in the picker', () => {
    expect(FONT_OPTIONS.some(o => o.value === DEFAULT_SETTINGS.fontFamily)).toBe(true);
  });
});


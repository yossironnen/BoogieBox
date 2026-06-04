/**
 * Tests Settings Page.Test behavior for BoogieBox regressions.
 */

import { describe, it, expect } from 'vitest';
import { hasSpotifyCredentials, isValidDlnaPort, THEME_PRESETS } from '../components/SettingsPage';

describe('hasSpotifyCredentials', () => {
  it('returns true when both client ID and secret are provided', () => {
    expect(hasSpotifyCredentials('abc123', 'secret456')).toBe(true);
  });

  it('returns false when either field is empty/whitespace', () => {
    expect(hasSpotifyCredentials('', 'secret456')).toBe(false);
    expect(hasSpotifyCredentials('abc123', '')).toBe(false);
    expect(hasSpotifyCredentials('   ', 'secret456')).toBe(false);
  });
});

describe('THEME_PRESETS', () => {
  it('includes existing built-in presets', () => {
    const labels = THEME_PRESETS.map(p => p.label);
    expect(labels).toContain('Dark (default)');
    expect(labels).toContain('Midnight Blue');
    expect(labels).toContain('Forest');
    expect(labels).toContain('Warm Dark');
    expect(labels).toContain('Vintage Radio');
    expect(labels).toContain('Light');
    expect(labels).toContain('Solarized');
    expect(labels).not.toContain('Original');
  });

  it('includes the requested groovy, classic, and modern themes with fonts', () => {
    const labels = THEME_PRESETS.map(p => p.label);
    expect(labels).toContain('Neon Groove');
    expect(labels).toContain('Disco Citrus');
    expect(labels).toContain('Ivory Ledger');
    expect(labels).toContain('Oxford Brass');
    expect(labels).toContain('Graphite Mint');

    const modern = THEME_PRESETS.find(p => p.label === 'Graphite Mint');
    expect(modern?.settings.fontFamily).toBeTruthy();
  });

  it('includes Vintage Radio with wood background texture', () => {
    const vintage = THEME_PRESETS.find(p => p.label === 'Vintage Radio');
    expect(vintage).toBeTruthy();
    expect(vintage?.settings.bgTexture).toBe('wood');
  });
});

describe('isValidDlnaPort', () => {
  it('returns true for valid integer ports in the allowed range', () => {
    expect(isValidDlnaPort('1024')).toBe(true);
    expect(isValidDlnaPort('8200')).toBe(true);
    expect(isValidDlnaPort('65535')).toBe(true);
  });

  it('returns false for non-integer or out-of-range values', () => {
    expect(isValidDlnaPort('1023')).toBe(false);
    expect(isValidDlnaPort('65536')).toBe(false);
    expect(isValidDlnaPort('not-a-number')).toBe(false);
    expect(isValidDlnaPort('80.5')).toBe(false);
  });
});

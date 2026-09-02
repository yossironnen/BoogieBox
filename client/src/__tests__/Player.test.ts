/**
 * Tests Player.Test behavior for BoogieBox regressions.
 */

import { describe, it, expect } from 'vitest';
import {
  fmt,
  getTranscodeWarning,
  getTranscodeFallbackUrl,
  getPreferredTrackStreamUrl,
  isBoogieMixSyntheticTrackId,
  buildPlaybackDebugInfo,
  beginRoundedRect,
  getSyntheticVuLevel,
  shouldResumeAudioContext,
  truncateTrackTitle,
  PLAYER_LAYOUT,
  PLAYER_THEME_TOKENS,
  DARK_DEFAULT_NEEDLE_PALETTE,
  DARK_DEFAULT_HIFI_PALETTE,
  normalizeVizMode,
  getNextVizMode,
  getVizModeToggleTitle,
  resolveNeedleMeterPalette,
  resolveHifiMeterPalette,
  computeTransitionThreshold,
  clampCrossfadeDuration,
  crossfadeVolumeAt,
  shouldPreserveTransitionPlayback,
} from '../components/Player';
import { DEFAULT_SETTINGS } from '../types';
import { setStreamDirect } from '../api';

// ── fmt ──────────────────────────────────────────────────────────────────────

describe('fmt', () => {
  it('formats zero as 0:00', () => {
    expect(fmt(0)).toBe('0:00');
  });

  it('pads seconds below 10 with a leading zero', () => {
    expect(fmt(9)).toBe('0:09');
    expect(fmt(65)).toBe('1:05');
  });

  it('formats whole minutes', () => {
    expect(fmt(60)).toBe('1:00');
    expect(fmt(120)).toBe('2:00');
    expect(fmt(3600)).toBe('60:00');
  });

  it('formats arbitrary durations', () => {
    expect(fmt(3661)).toBe('61:01');
    expect(fmt(90)).toBe('1:30');
    expect(fmt(599)).toBe('9:59');
  });

  it('returns 0:00 for NaN', () => {
    expect(fmt(NaN)).toBe('0:00');
  });

  it('returns 0:00 for +Infinity', () => {
    expect(fmt(Infinity)).toBe('0:00');
  });

  it('returns 0:00 for -Infinity', () => {
    expect(fmt(-Infinity)).toBe('0:00');
  });
});

// ── getTranscodeWarning ───────────────────────────────────────────────────────

describe('getTranscodeWarning', () => {
  it('returns null for a null track', () => {
    expect(getTranscodeWarning(null, null)).toBeNull();
  });

  it('returns null for a track with no file name or path', () => {
    expect(getTranscodeWarning({} as any, true)).toBeNull();
  });

  it('returns null for native browser formats (mp3)', () => {
    expect(getTranscodeWarning({ file_path: '/music/song.mp3' } as any, true)).toBeNull();
  });

  it('returns null for native browser formats (wav, ogg, opus)', () => {
    ['.wav', '.ogg', '.opus'].forEach(ext => {
      expect(getTranscodeWarning({ file_path: `/music/track${ext}` } as any, true)).toBeNull();
    });
  });

  it('returns null for FLAC when ffmpeg IS available', () => {
    expect(getTranscodeWarning({ file_path: '/music/song.flac' } as any, true)).toBeNull();
  });

  it('returns null for FLAC when ffmpeg status is unknown', () => {
    expect(getTranscodeWarning({ file_path: '/music/song.flac' } as any, null)).toBeNull();
  });

  it('returns a warning for FLAC when ffmpeg is NOT available', () => {
    const warn = getTranscodeWarning({ file_path: '/music/song.flac' } as any, false);
    expect(warn).not.toBeNull();
    expect(warn).toContain('FLAC');
    expect(warn).toContain('ffmpeg');
  });

  it('uses file_name when API payloads omit file_path', () => {
    const warn = getTranscodeWarning({ file_name: 'song.flac' } as any, false);
    expect(warn).toContain('FLAC');
  });

  it('includes the correct format name in the warning', () => {
    const formats = ['.flac', '.m4a', '.wma', '.alac', '.ape', '.aiff'];
    formats.forEach(ext => {
      const warn = getTranscodeWarning({ file_path: `/music/track${ext}` } as any, false);
      expect(warn).toContain(ext.slice(1).toUpperCase());
    });
  });
});

describe('getTranscodeFallbackUrl', () => {
  it('returns null when noTranscode is absent', () => {
    expect(getTranscodeFallbackUrl('/api/tracks/1/stream')).toBeNull();
    expect(getTranscodeFallbackUrl('/api/tracks/1/stream?x=1')).toBeNull();
  });

  it('removes noTranscode=1 from a relative URL', () => {
    expect(getTranscodeFallbackUrl('/api/tracks/1/stream?noTranscode=1')).toBe('/api/tracks/1/stream');
  });

  it('preserves other query params when removing noTranscode=1', () => {
    expect(getTranscodeFallbackUrl('/api/tracks/1/stream?foo=bar&noTranscode=1&x=2'))
      .toBe('/api/tracks/1/stream?foo=bar&x=2');
  });

  it('removes noTranscode=1 from an absolute URL', () => {
    expect(getTranscodeFallbackUrl('http://localhost:3001/api/tracks/1/stream?noTranscode=1'))
      .toBe('http://localhost:3001/api/tracks/1/stream');
  });

  it('returns null for empty input', () => {
    expect(getTranscodeFallbackUrl('')).toBeNull();
  });
});

describe('getPreferredTrackStreamUrl', () => {
  it('ignores direct-stream preference for FLAC so the server transcodes it', () => {
    setStreamDirect(true);
    expect(getPreferredTrackStreamUrl({
      id: '42',
      file_name: 'song.flac',
    } as any)).toBe('/api/tracks/42/stream');
    setStreamDirect(false);
  });

  it('keeps direct-stream preference for browser-native formats', () => {
    setStreamDirect(true);
    expect(getPreferredTrackStreamUrl({
      id: '43',
      file_name: 'song.mp3',
    } as any)).toBe('/api/tracks/43/stream?noTranscode=1');
    setStreamDirect(false);
  });

  it('prefers stream_url_override when present (BoogieMix output synthetic track)', () => {
    expect(getPreferredTrackStreamUrl({
      id: 'boogiemix:output-1',
      file_name: 'mix.mp3',
      stream_url_override: '/api/boogiemix/outputs/output-1/play',
    } as any)).toBe('/api/boogiemix/outputs/output-1/play');
  });
});

describe('isBoogieMixSyntheticTrackId', () => {
  it('flags boogiemix:-prefixed ids and passes through real track ids', () => {
    expect(isBoogieMixSyntheticTrackId('boogiemix:output-1')).toBe(true);
    expect(isBoogieMixSyntheticTrackId('42')).toBe(false);
    expect(isBoogieMixSyntheticTrackId(undefined)).toBe(false);
    expect(isBoogieMixSyntheticTrackId(null)).toBe(false);
  });
});

describe('buildPlaybackDebugInfo', () => {
  it('returns normalized playback debug details when track exists', () => {
    expect(buildPlaybackDebugInfo({
      id: '42',
      title: 'Track A',
      file_name: 'track-a.flac',
      file_path: 'C:\\Music\\track-a.FLAC',
    } as any, '/api/tracks/42/stream')).toEqual({
      trackId: '42',
      title: 'Track A',
      ext: '.flac',
      url: '/api/tracks/42/stream',
    });
  });

  it('handles missing track/file path safely', () => {
    expect(buildPlaybackDebugInfo(null, '/api/tracks/1/stream')).toEqual({
      trackId: null,
      title: '',
      ext: '',
      url: '/api/tracks/1/stream',
    });
  });

  it('builds debug extensions from file_name without local paths', () => {
    expect(buildPlaybackDebugInfo({
      id: '43',
      title: 'Track B',
      file_name: 'track-b.m4a',
    } as any, '/api/tracks/43/stream')).toEqual({
      trackId: '43',
      title: 'Track B',
      ext: '.m4a',
      url: '/api/tracks/43/stream',
    });
  });
});

describe('beginRoundedRect', () => {
  it('uses native roundRect when available', () => {
    const calls: string[] = [];
    const ctx = {
      beginPath: () => calls.push('beginPath'),
      roundRect: () => calls.push('roundRect'),
      moveTo: () => calls.push('moveTo'),
      lineTo: () => calls.push('lineTo'),
      arcTo: () => calls.push('arcTo'),
      closePath: () => calls.push('closePath'),
    } as unknown as CanvasRenderingContext2D;

    beginRoundedRect(ctx, 0, 0, 20, 10, 3);

    expect(calls).toContain('beginPath');
    expect(calls).toContain('roundRect');
    expect(calls).not.toContain('arcTo');
  });

  it('falls back to path drawing when roundRect is unavailable', () => {
    const calls: string[] = [];
    const ctx = {
      beginPath: () => calls.push('beginPath'),
      moveTo: () => calls.push('moveTo'),
      lineTo: () => calls.push('lineTo'),
      arcTo: () => calls.push('arcTo'),
      closePath: () => calls.push('closePath'),
    } as unknown as CanvasRenderingContext2D;

    beginRoundedRect(ctx, 0, 0, 20, 10, 3);

    expect(calls).toContain('beginPath');
    expect(calls).toContain('moveTo');
    expect(calls.filter(c => c === 'arcTo').length).toBe(4);
    expect(calls).toContain('closePath');
  });
});

describe('getSyntheticVuLevel', () => {
  it('returns null when not playing', () => {
    expect(getSyntheticVuLevel(false, false, 1, 'left')).toBeNull();
  });

  it('returns null when analyser is present', () => {
    expect(getSyntheticVuLevel(true, true, 1, 'left')).toBeNull();
  });

  it('returns a bounded synthetic level only when analyser is missing', () => {
    const levelL = getSyntheticVuLevel(true, false, 1, 'left');
    const levelR = getSyntheticVuLevel(true, false, 1, 'right');

    expect(levelL).not.toBeNull();
    expect(levelR).not.toBeNull();
    expect(levelL!).toBeGreaterThanOrEqual(0.04);
    expect(levelL!).toBeLessThanOrEqual(0.16);
    expect(levelR!).toBeGreaterThanOrEqual(0.04);
    expect(levelR!).toBeLessThanOrEqual(0.16);
    expect(levelL).not.toBe(levelR);
  });
});

describe('shouldResumeAudioContext', () => {
  it('returns false only for running state', () => {
    expect(shouldResumeAudioContext('running')).toBe(false);
  });

  it('returns true for suspended or interrupted-like states', () => {
    expect(shouldResumeAudioContext('suspended')).toBe(true);
    expect(shouldResumeAudioContext('interrupted')).toBe(true);
    expect(shouldResumeAudioContext('closed')).toBe(true);
  });
});

describe('viz mode helpers', () => {
  it('normalizes persisted viz mode values', () => {
    expect(normalizeVizMode('bars')).toBe('bars');
    expect(normalizeVizMode('needle')).toBe('needle');
    expect(normalizeVizMode('hifi')).toBe('hifi');
    expect(normalizeVizMode('wave')).toBe('wave');
    expect(normalizeVizMode('tube')).toBe('bars'); // legacy value falls back
    expect(normalizeVizMode('unknown')).toBe('bars');
    expect(normalizeVizMode(null)).toBe('bars');
  });

  it('cycles visualizer modes bars -> needle -> hifi -> wave -> bars', () => {
    expect(getNextVizMode('bars')).toBe('needle');
    expect(getNextVizMode('needle')).toBe('hifi');
    expect(getNextVizMode('hifi')).toBe('wave');
    expect(getNextVizMode('wave')).toBe('bars');
  });

  it('returns mode-toggle titles for each visualizer', () => {
    expect(getVizModeToggleTitle('bars')).toContain('needle');
    expect(getVizModeToggleTitle('needle')).toContain('HiFi');
    expect(getVizModeToggleTitle('hifi')).toContain('visualizer');
    expect(getVizModeToggleTitle('wave')).toContain('bar');
  });
});

describe('resolveNeedleMeterPalette', () => {
  it('returns exact legacy palette for the current default theme values', () => {
    const palette = resolveNeedleMeterPalette({
      bg: DEFAULT_SETTINGS.colorBg,
      surface: DEFAULT_SETTINGS.colorSurface,
      border: DEFAULT_SETTINGS.colorBorder,
      accent: DEFAULT_SETTINGS.colorAccent,
      text: DEFAULT_SETTINGS.colorText,
      textMuted: DEFAULT_SETTINGS.colorTextMuted,
    });

    expect(palette).toEqual(DARK_DEFAULT_NEEDLE_PALETTE);
  });

  it('derives a different needle palette for non-default themes', () => {
    const palette = resolveNeedleMeterPalette({
      bg: '#f8f8f8',
      surface: '#ffffff',
      border: '#e4e4e7',
      accent: '#6366f1',
      text: '#18181b',
      textMuted: '#71717a',
    });

    expect(palette).not.toEqual(DARK_DEFAULT_NEEDLE_PALETTE);
    expect(palette.plateTop).not.toBe(DARK_DEFAULT_NEEDLE_PALETTE.plateTop);
    expect(palette.vuLabel).not.toBe(DARK_DEFAULT_NEEDLE_PALETTE.vuLabel);
    expect(palette.arcGreenStart).toBe(DARK_DEFAULT_NEEDLE_PALETTE.arcGreenStart);
    expect(palette.arcYellowEnd).toBe(DARK_DEFAULT_NEEDLE_PALETTE.arcYellowEnd);
    expect(palette.arcRedEnd).toBe(DARK_DEFAULT_NEEDLE_PALETTE.arcRedEnd);
  });
});

describe('resolveHifiMeterPalette', () => {
  it('returns exact HiFi dark-default palette for the current default theme values', () => {
    const palette = resolveHifiMeterPalette({
      bg: DEFAULT_SETTINGS.colorBg,
      surface: DEFAULT_SETTINGS.colorSurface,
      border: DEFAULT_SETTINGS.colorBorder,
      accent: DEFAULT_SETTINGS.colorAccent,
      text: DEFAULT_SETTINGS.colorText,
      textMuted: DEFAULT_SETTINGS.colorTextMuted,
    });

    expect(palette).toEqual(DARK_DEFAULT_HIFI_PALETTE);
  });

  it('derives a themed HiFi palette for non-default themes', () => {
    const palette = resolveHifiMeterPalette({
      bg: '#f8f8f8',
      surface: '#ffffff',
      border: '#e4e4e7',
      accent: '#6366f1',
      text: '#18181b',
      textMuted: '#71717a',
    });

    expect(palette).not.toEqual(DARK_DEFAULT_HIFI_PALETTE);
    expect(palette.dialCenter).not.toBe(DARK_DEFAULT_HIFI_PALETTE.dialCenter);
    expect(palette.frameTop).not.toBe(DARK_DEFAULT_HIFI_PALETTE.frameTop);
    expect(palette.needleCore).not.toBe(DARK_DEFAULT_HIFI_PALETTE.needleCore);
  });
});

describe('truncateTrackTitle', () => {
  it('returns the original title when at or below max length', () => {
    expect(truncateTrackTitle('Short title', 35)).toBe('Short title');
    expect(truncateTrackTitle('12345678901234567890123456789012345', 35))
      .toBe('12345678901234567890123456789012345');
  });

  it('truncates long titles and appends three dots', () => {
    expect(truncateTrackTitle('1234567890123456789012345678901234567890', 35))
      .toBe('12345678901234567890123456789012...');
  });

  it('handles very small max lengths', () => {
    expect(truncateTrackTitle('abcdef', 3)).toBe('...');
    expect(truncateTrackTitle('abcdef', 2)).toBe('..');
    expect(truncateTrackTitle('abcdef', 0)).toBe('');
  });
});

describe('PLAYER_THEME_TOKENS', () => {
  it('maps player pane styles to app theme CSS variables', () => {
    expect(PLAYER_THEME_TOKENS.bg).toBe('var(--bg)');
    expect(PLAYER_THEME_TOKENS.surface).toBe('var(--surface)');
    expect(PLAYER_THEME_TOKENS.border).toBe('var(--border)');
    expect(PLAYER_THEME_TOKENS.accent).toBe('var(--accent)');
    expect(PLAYER_THEME_TOKENS.text).toBe('var(--text)');
    expect(PLAYER_THEME_TOKENS.textMuted).toBe('var(--text-muted)');
    expect(PLAYER_THEME_TOKENS.font).toContain('var(--font)');
  });
});

describe('PLAYER_LAYOUT', () => {
  it('uses constrained track info width and a capped flexible progress area', () => {
    expect(PLAYER_LAYOUT.trackTitleMaxChars).toBe(35);
    expect(PLAYER_LAYOUT.trackInfoMinWidth).toBeGreaterThanOrEqual(160);
    expect(PLAYER_LAYOUT.trackInfoMaxWidth).toBeGreaterThan(PLAYER_LAYOUT.trackInfoMinWidth);
    expect(PLAYER_LAYOUT.progressMinWidth).toBeGreaterThanOrEqual(160);
    expect(PLAYER_LAYOUT.progressWidth).toBe('36vw');
    expect(PLAYER_LAYOUT.progressMaxWidth).toBe(460);
  });
});

// ── computeTransitionThreshold ──────────────────────────────────────────────

describe('computeTransitionThreshold', () => {
  it('returns 0 when mode is off', () => {
    expect(computeTransitionThreshold('off', 5)).toBe(0);
  });

  it('returns 0 when mode is off regardless of crossfadeDuration', () => {
    expect(computeTransitionThreshold('off', 0)).toBe(0);
    expect(computeTransitionThreshold('off', 10)).toBe(0);
    expect(computeTransitionThreshold('off', 100)).toBe(0);
  });

  it('returns 0.3 when mode is zerogap', () => {
    expect(computeTransitionThreshold('zerogap', 5)).toBe(0.3);
  });

  it('returns 0.3 when mode is zerogap regardless of crossfadeDuration', () => {
    expect(computeTransitionThreshold('zerogap', 0)).toBe(0.3);
    expect(computeTransitionThreshold('zerogap', 15)).toBe(0.3);
  });

  it('returns the crossfadeDuration when mode is crossfade', () => {
    expect(computeTransitionThreshold('crossfade', 5)).toBe(5);
    expect(computeTransitionThreshold('crossfade', 10)).toBe(10);
  });

  it('returns 0 crossfadeDuration for crossfade mode when duration is 0', () => {
    expect(computeTransitionThreshold('crossfade', 0)).toBe(0);
  });
});

// ── clampCrossfadeDuration ──────────────────────────────────────────────────

describe('clampCrossfadeDuration', () => {
  it('returns the requested duration when track is long enough', () => {
    expect(clampCrossfadeDuration(5, 60)).toBe(5);
    expect(clampCrossfadeDuration(10, 30)).toBe(10);
  });

  it('clamps to half the track duration when requested exceeds half', () => {
    // trackDuration=8 → maxAllowed = floor(8/2) = 4
    expect(clampCrossfadeDuration(5, 8)).toBe(4);
    expect(clampCrossfadeDuration(10, 6)).toBe(3);
  });

  it('returns 0 when track duration is 0', () => {
    expect(clampCrossfadeDuration(5, 0)).toBe(0);
  });

  it('returns 0 when track duration is negative', () => {
    expect(clampCrossfadeDuration(5, -10)).toBe(0);
  });

  it('guarantees at least 1 when track is very short but positive', () => {
    // trackDuration=1 → maxAllowed = floor(1/2) = 0, but max(0, 1) = 1
    expect(clampCrossfadeDuration(5, 1)).toBe(1);
  });

  it('handles odd track durations correctly', () => {
    // trackDuration=7 → maxAllowed = floor(7/2) = 3
    expect(clampCrossfadeDuration(5, 7)).toBe(3);
    expect(clampCrossfadeDuration(3, 7)).toBe(3);
    expect(clampCrossfadeDuration(2, 7)).toBe(2);
  });

  it('returns requested duration when it equals exactly half', () => {
    // trackDuration=10 → maxAllowed = 5
    expect(clampCrossfadeDuration(5, 10)).toBe(5);
  });
});

// ── crossfadeVolumeAt ───────────────────────────────────────────────────────

describe('crossfadeVolumeAt', () => {
  describe('direction: in (fade in)', () => {
    it('returns 0 at progress 0', () => {
      expect(crossfadeVolumeAt(0, 1, 'in')).toBe(0);
    });

    it('returns half of targetVolume at progress 0.5', () => {
      expect(crossfadeVolumeAt(0.5, 1, 'in')).toBe(0.5);
      expect(crossfadeVolumeAt(0.5, 0.8, 'in')).toBeCloseTo(0.4);
    });

    it('returns targetVolume at progress 1', () => {
      expect(crossfadeVolumeAt(1, 1, 'in')).toBe(1);
      expect(crossfadeVolumeAt(1, 0.6, 'in')).toBeCloseTo(0.6);
    });

    it('scales linearly with targetVolume', () => {
      expect(crossfadeVolumeAt(0.25, 2, 'in')).toBeCloseTo(0.5);
    });
  });

  describe('direction: out (fade out)', () => {
    it('returns targetVolume at progress 0', () => {
      expect(crossfadeVolumeAt(0, 1, 'out')).toBe(1);
      expect(crossfadeVolumeAt(0, 0.7, 'out')).toBeCloseTo(0.7);
    });

    it('returns half of targetVolume at progress 0.5', () => {
      expect(crossfadeVolumeAt(0.5, 1, 'out')).toBe(0.5);
      expect(crossfadeVolumeAt(0.5, 0.8, 'out')).toBeCloseTo(0.4);
    });

    it('returns 0 at progress 1', () => {
      expect(crossfadeVolumeAt(1, 1, 'out')).toBe(0);
      expect(crossfadeVolumeAt(1, 0.9, 'out')).toBe(0);
    });
  });

  describe('progress clamping', () => {
    it('clamps progress below 0 to 0', () => {
      expect(crossfadeVolumeAt(-0.5, 1, 'in')).toBe(0);
      expect(crossfadeVolumeAt(-1, 1, 'out')).toBe(1);
    });

    it('clamps progress above 1 to 1', () => {
      expect(crossfadeVolumeAt(1.5, 1, 'in')).toBe(1);
      expect(crossfadeVolumeAt(2, 1, 'out')).toBe(0);
    });
  });

  describe('zero targetVolume', () => {
    it('returns 0 for any progress when targetVolume is 0', () => {
      expect(crossfadeVolumeAt(0, 0, 'in')).toBe(0);
      expect(crossfadeVolumeAt(0.5, 0, 'in')).toBe(0);
      expect(crossfadeVolumeAt(1, 0, 'in')).toBe(0);
      expect(crossfadeVolumeAt(0, 0, 'out')).toBe(0);
      expect(crossfadeVolumeAt(0.5, 0, 'out')).toBe(0);
      expect(crossfadeVolumeAt(1, 0, 'out')).toBe(0);
    });
  });
});

describe('shouldPreserveTransitionPlayback', () => {
  it('returns false when handoff is not active', () => {
    expect(shouldPreserveTransitionPlayback({
      handoffInProgress: false,
      expectedUrl: '/api/tracks/2/stream',
      loadedUrl: '/api/tracks/2/stream',
      currentAudioSrc: '/api/tracks/2/stream',
    })).toBe(false);
  });

  it('returns true when loaded URL already matches expected handoff URL', () => {
    expect(shouldPreserveTransitionPlayback({
      handoffInProgress: true,
      expectedUrl: '/api/tracks/2/stream',
      loadedUrl: '/api/tracks/2/stream',
      currentAudioSrc: '/api/tracks/1/stream',
    })).toBe(true);
  });

  it('returns true when current audio source matches expected after URL normalization', () => {
    expect(shouldPreserveTransitionPlayback({
      handoffInProgress: true,
      expectedUrl: '/api/tracks/2/stream',
      loadedUrl: '/api/tracks/1/stream',
      currentAudioSrc: 'http://localhost:3000/api/tracks/2/stream',
    })).toBe(true);
  });

  it('returns false when neither loaded nor current audio source matches expected', () => {
    expect(shouldPreserveTransitionPlayback({
      handoffInProgress: true,
      expectedUrl: '/api/tracks/3/stream',
      loadedUrl: '/api/tracks/2/stream',
      currentAudioSrc: '/api/tracks/1/stream',
    })).toBe(false);
  });
});
